use crate::assembly::{
    AssemblyEditorState, AssemblyGizmoMode, reference_size as assembly_reference_size,
};
use crate::measurement::{MeasurementBoxState, MeasurementTarget};
use crate::slider::{SliderConfig, SliderId, SliderState, SliderTrack, spawn_slider};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::ScrollPosition;
use camera::OrbitCamera;
use fem_core::{
    ContactCandidateState, ContactPair, ContactSlaveRef, ContactType, FemEntityId, FemEntityRef,
    FemModel, FemModelVersion, FemNodeSet, FemResultSet, FemSurfaceSet, MeshLoadRequest,
    MeshLoadStatus, SelectionFilter, SelectionLevel, UiPointerState, ViewportTool,
};
use interaction::HoverResult;
use selection::{Hovered, Selectable, Selected, SelectionOperation, SelectionState};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use visualization::ContourSettings;
use visualization::{
    BoundaryLoadPreview, BoundaryLoadPreviewArrow, BoundaryLoadPreviewKind, ContactDraftPreview,
    ContactDraftSlave, ContactDraftSurface, ContactReviewSettings, DefinedContactPreview,
    VisualizationMode, VisualizationSettings,
};

const PANEL_BG: Color = Color::srgba(0.035, 0.04, 0.045, 0.88);
const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);
const ACTIVE_BORDER: Color = Color::srgb(0.57, 0.86, 0.92);
const CONTACT_MASTER_BUTTON: Color = Color::srgb(0.16, 0.34, 0.60);
const CONTACT_SLAVE_BUTTON: Color = Color::srgb(0.62, 0.31, 0.08);
const COPLANAR_TOLERANCE_DEG: f32 = 0.5;
const DEFAULT_SMOOTH_ANGLE_DEG: f32 = 15.0;
const SELECTION_GUIDE_TEXT: &str = "Click / drag       Replace selection\n\
Double click       Connected boundary\n\
Triple click       Connected component\n\
Ctrl + click/drag  Add to selection\n\
Shift + click/drag Toggle selected / unselected\n\
Alt or Ctrl+Shift  Remove from selection\n\
Esc                Clear all\n\
Drag left → right  Fully enclosed only\n\
Drag right → left  Crossing / touching";

#[derive(Component)]
pub(crate) struct SelectionLevelButton {
    level: SelectionLevel,
}

#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct SelectionGuideState {
    pub expanded: bool,
}

impl Default for SelectionGuideState {
    fn default() -> Self {
        Self { expanded: true }
    }
}

#[derive(Component)]
pub(crate) struct SelectionGuideToggle;

#[derive(Component)]
pub(crate) struct SelectionGuidePanel;

#[derive(Component)]
pub(crate) struct SelectionToolbar;

#[derive(Component)]
pub(crate) struct SelectionContextText;

#[derive(Component)]
pub(crate) struct SelectionOperationHint;

#[derive(Component)]
pub(crate) struct RenderModeButton {
    mode: VisualizationMode,
}

#[derive(Component)]
pub(crate) struct OpenMeshButton;

#[derive(Component)]
pub(crate) struct ImportMeshButton;

/// Opens `hecmw_ctrl.dat`, reads mesh/cnt stems, and loads both files in
/// one click — the "Open Project" shortcut.
#[derive(Component)]
pub(crate) struct OpenProjectButton;

/// Saves currently selected nodes as a new [`fem_core::FemNodeSet`].
#[derive(Component)]
pub(crate) struct MakeNodeGroupButton;

/// Saves currently selected elements as a new [`fem_core::FemElementSet`].
#[derive(Component)]
pub(crate) struct MakeElementGroupButton;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SurfaceSelectionMode {
    #[default]
    Single,
    Coplanar,
    Smooth,
}

impl SurfaceSelectionMode {
    const fn label(self) -> &'static str {
        match self {
            Self::Single => "Single",
            Self::Coplanar => "Coplanar",
            Self::Smooth => "Smooth",
        }
    }
}

/// Controls how one hovered boundary face grows into a selection preview.
///
/// `Coplanar` compares every face with the seed normal; `Smooth` compares
/// neighbouring faces and can therefore follow a curved surface. The target
/// type remains pure: Face mode yields faces, while Element mode yields the
/// complete elements immediately behind the same surface patch.
#[derive(Resource, Debug, Clone)]
pub(crate) struct SurfaceSelectionSettings {
    pub mode: SurfaceSelectionMode,
}

impl Default for SurfaceSelectionSettings {
    fn default() -> Self {
        Self {
            mode: SurfaceSelectionMode::Single,
        }
    }
}

#[derive(Component)]
pub(crate) struct SurfaceSelectionModeButton {
    mode: SurfaceSelectionMode,
}

#[derive(Component)]
pub(crate) struct SurfaceSelectionHint;

#[derive(Component)]
pub(crate) struct SurfaceAngleControls;

#[derive(Component)]
pub(crate) struct SurfaceSelectionControls;

#[derive(Component)]
pub(crate) struct SurfaceSelectionUnavailableHint;

/// Active section type selection for the "Add Section" panel.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SelectedSectionType {
    #[default]
    Solid,
    Shell,
    Beam,
}

/// Active element group name for the "Add Section" panel.
/// `None` means "ALL" (no EGRP restriction).
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct SelectedEgrp(pub Option<String>);

/// Active material name for the "Add Section" panel.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct SelectedMaterialForSection(pub Option<String>);

/// Marks a section-type toggle button ([Solid] / [Shell] / [Beam]).
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SectionTypeButton(pub SelectedSectionType);

/// Marks an EGRP selection button in the section definition panel.
#[derive(Component, Debug, Clone)]
pub(crate) struct EgrpSelectButton(pub Option<String>);

/// Marks a material selection button in the section definition panel.
#[derive(Component, Debug, Clone)]
pub(crate) struct MaterialSelectButton(pub String);

/// Adds the configured section to [`fem_core::AnalysisSetup`].
#[derive(Component)]
pub(crate) struct AddSectionButton;

/// The SliderId for the section thickness / beam area input.
// (Reuses the existing slider machinery — no new slider type needed.)
// Handled via SliderId::SectionThickness added in slider.rs.

/// Marks the container that [`rebuild_section_def_panel`] fills with the
/// current EGRP and material buttons whenever the model or setup changes.
#[derive(Component)]
pub(crate) struct SectionDefEgrpRow;

#[derive(Component)]
pub(crate) struct SectionDefMatRow;

#[derive(Component)]
pub(crate) struct CreateSurfaceButton;

#[derive(Component)]
pub(crate) struct CreateContactButton;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ContactPairKind {
    #[default]
    NodeSurface,

    SurfaceSurface,
}

impl ContactPairKind {
    const ALL: [Self; 2] = [Self::NodeSurface, Self::SurfaceSurface];

    const fn label(self) -> &'static str {
        match self {
            Self::NodeSurface => "NODE-SURF",
            Self::SurfaceSurface => "SURF-SURF",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ContactParameter {
    #[default]
    Friction,

    PenaltyFactor,
}

#[derive(Resource, Debug, Clone)]
pub(crate) struct ContactDefinitionSettings {
    pub pair_kind: ContactPairKind,

    pub contact_type: ContactType,

    pub use_penalty_factor: bool,

    pub active_parameter: ContactParameter,

    pub message: String,
}

impl Default for ContactDefinitionSettings {
    fn default() -> Self {
        Self {
            pair_kind: ContactPairKind::NodeSurface,
            contact_type: ContactType::SmallSliding,
            use_penalty_factor: false,
            active_parameter: ContactParameter::Friction,
            message: "Select slave nodes, then capture Slave".to_string(),
        }
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ContactPairKindButton(pub ContactPairKind);

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ContactBehaviorButton(pub ContactType);

#[derive(Component)]
pub(crate) struct ContactSlidingParameterControls;

#[derive(Component)]
pub(crate) struct ContactPenaltyControls;

#[derive(Component)]
pub(crate) struct ContactPenaltyToggleButton;

#[derive(Component)]
pub(crate) struct ContactPenaltyToggleLabel;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ContactParameterButton(pub ContactParameter);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContactCaptureSide {
    Slave,

    Master,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct CaptureContactSideButton(pub ContactCaptureSide);

#[derive(Component)]
pub(crate) struct FinalizeContactButton;

#[derive(Component)]
pub(crate) struct ContactDraftStatusText;

#[derive(Component)]
pub(crate) struct DetectContactsButton;

#[derive(Component)]
pub(crate) struct AcceptContactButton;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContactCandidateAction {
    Previous,

    Next,

    Reject,
}

#[derive(Component)]
pub(crate) struct ContactCandidateActionButton(pub ContactCandidateAction);

#[derive(Component)]
pub(crate) struct ContactGhostToggleButton;

#[derive(Component)]
pub(crate) struct ContactGhostToggleLabel;

#[derive(Component)]
pub(crate) struct MeshStatsText;

#[derive(Component)]
pub(crate) struct SelectionStatsText;

#[derive(Component)]
pub(crate) struct ContactCandidateText;

#[derive(Component)]
pub(crate) struct ContactDefinitionsText;

#[derive(Component)]
pub(crate) struct ContactDefinitionsListContainer;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct DefinedContactButton(pub usize);

#[derive(Component)]
pub(crate) struct ContactReviewControls;

#[derive(Component)]
pub(crate) struct OpenResultButton;

#[derive(Component)]
pub(crate) struct ResultStatsText;

#[derive(Component)]
pub(crate) struct ResultSliderSection;

/// Animation playback state for automatic result step advancement.
#[derive(Resource, Debug, Clone)]
pub(crate) struct PlaybackState {
    pub playing: bool,
    /// Seconds per step (0.1 = 10fps, 0.5 = 2fps).
    pub interval: f32,
    /// Accumulator — elapsed time since last step advance.
    pub elapsed: f32,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            playing: false,
            interval: 0.2,
            elapsed: 0.0,
        }
    }
}

/// ◀◀ rewind to first step.
#[derive(Component)]
pub(crate) struct PlaybackRewindButton;

/// ▶ / ‖ play-pause toggle.
#[derive(Component)]
pub(crate) struct PlaybackPlayPauseButton;

/// ▶▶ fast-forward to last step.
#[derive(Component)]
pub(crate) struct PlaybackEndButton;

/// Dynamic label on the play-pause button.
#[derive(Component)]
pub(crate) struct PlaybackPlayPauseLabel;

/// Speed slider for animation playback (`SliderId::PlaybackSpeed`).
// handled by the existing slider machinery; no extra component needed.

/// Flag set by [`undo_redo_system`] to suppress the snapshot system from
/// treating an undo/redo itself as a new user action that should be recorded.
#[derive(Resource, Default)]
pub(crate) struct UndoInProgress(pub bool);
///
/// Each user action that modifies [`AnalysisSetup`] pushes the *previous*
/// state onto the undo stack before applying the change. Ctrl+Z pops from
/// the undo stack (pushing the current state onto the redo stack); Ctrl+Y
/// / Ctrl+Shift+Z pops from redo.
///
/// Stack depth is capped at [`UndoStack::MAX_DEPTH`] to bound memory use.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct UndoStack {
    pub undo: Vec<fem_core::AnalysisSetup>,
    pub redo: Vec<fem_core::AnalysisSetup>,
}

impl UndoStack {
    /// Maximum number of undo states kept.
    pub const MAX_DEPTH: usize = 20;

    /// Pushes the current `setup` onto the undo stack and clears redo.
    /// Call this *before* applying any change to `setup`.
    pub fn push(&mut self, setup: fem_core::AnalysisSetup) {
        self.undo.push(setup);
        if self.undo.len() > Self::MAX_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
}

#[derive(Component)]
pub(crate) struct AssemblyPartsContainer;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AssemblyPartButton {
    part_index: usize,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AssemblyTransformButton {
    action: AssemblyTransformAction,
}

#[derive(Debug, Clone, Copy)]
enum AssemblyTransformAction {
    Translate(Vec3),
    Rotate(Vec3),
    Reset,
}

#[derive(Component)]
pub(crate) struct AssemblyStatusText;

#[derive(Component)]
pub(crate) struct AssemblyModeButton;

#[derive(Component)]
pub(crate) struct AssemblyModeButtonLabel;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AssemblyGizmoModeButton {
    mode: AssemblyGizmoMode,
}

/// Explicit camera-fit request, kept separate from [`FemModelVersion`] so
/// assembly edits can rebuild geometry without disrupting the current view.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct CameraFitRequest {
    revision: u64,
}

impl CameraFitRequest {
    fn request(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

#[derive(Component)]
pub(crate) struct OpenSetupButton;

#[derive(Component)]
pub(crate) struct ExportButton;

#[derive(Component)]
pub(crate) struct ExportStatusText;

#[derive(Component)]
pub(crate) struct AnalysisSetupStatsText;

#[derive(Component)]
pub(crate) struct ToggleConstraintsButton;

#[derive(Component)]
pub(crate) struct ToggleLoadsButton;

/// Which degrees of freedom a [`ConstraintPresetButton`] fixes, expressed
/// as a `[dof_start, dof_end]` range in the standard FEM numbering
/// (`1=Ux, 2=Uy, 3=Uz, 4=Rx, 5=Ry, 6=Rz`) — see
/// [`fem_core::BoundaryCondition`].
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ConstraintPresetButton {
    pub dof_start: u8,
    pub dof_end: u8,
    pub label: &'static str,
}

/// One of the six axis-aligned load directions offered by the load-creation
/// panel; `dof` is the standard FEM force numbering (`1=Fx, 2=Fy, 3=Fz`)
/// and `sign` flips the magnitude for the negative-axis buttons.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct LoadDirectionButton {
    pub dof: u8,
    pub sign: f32,
}

/// Tracks which [`LoadDirectionButton`] is currently selected, so the
/// magnitude slider and "Apply Load" button know which DOF/sign to use.
/// `None` until the person picks a direction.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct SelectedLoadDirection(pub Option<(u8, f32)>);

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ActiveLoadEditor {
    #[default]
    None,

    Nodal,

    Distributed,
}

#[derive(Component)]
pub(crate) struct ApplyLoadButton;

/// Dynamic text below the SELECTION section showing:
/// "N nodes | M elements selected  ·  hover: NodeID (x, y, z)"
/// Updated every frame by [`update_selection_info_text`].
#[derive(Component)]
pub(crate) struct SelectionInfoText;

/// The text label inside a [`ConstraintPresetButton`]; updated dynamically
/// by [`update_constraint_button_labels`] to show the current selected-node
/// count, e.g. "Fix XYZ (37)".  When 0 nodes are selected the button is
/// dimmed to signal that clicking it would have no effect.
#[derive(Component)]
pub(crate) struct ConstraintPresetLabel;

/// Marks the "Apply Load" button's text child for dynamic label updates
/// ("Apply Load (37)" / "Apply Load — no nodes selected").
#[derive(Component)]
pub(crate) struct ApplyLoadLabel;

/// Clears all boundary conditions AND all nodal loads in one click.
/// Useful for "start over" after a wrong preset was applied to many nodes.
#[derive(Component)]
pub(crate) struct ClearAllBcLoadsButton;

/// Active distributed-load kind for the "Add Distributed Load" panel.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SelectedDloadKind {
    #[default]
    Pressure,
    Gravity,
}

/// Marks a [DistributedLoadKind] toggle button ([Pressure] / [Gravity]) in
/// the Add Distributed Load panel.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct DloadKindButton(pub SelectedDloadKind);

/// Applies the configured distributed load (pressure or gravity) to every
/// currently selected face's parent element.
#[derive(Component)]
pub(crate) struct ApplyDloadButton;

/// Dynamic label inside [`ApplyDloadButton`], showing the face/element count.
#[derive(Component)]
pub(crate) struct ApplyDloadLabel;

/// Marks an [`fem_core::AnalysisType`] toggle button in the solver panel.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AnalysisTypeButton(pub fem_core::AnalysisType);

/// Marks a [`fem_core::LinearSolverMethod`] toggle button.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SolverMethodButton(pub fem_core::LinearSolverMethod);

/// One built-in material preset (name + properties), offered as a single
/// "+ Steel"-style button so creating a common material needs no numeric
/// text entry — see [`material_presets`].
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct MaterialPresetButton {
    pub preset_index: usize,
}

/// On a delete ("✕") button spawned next to a list entry, identifies which
/// entry to remove and from which list — used by
/// [`delete_setup_entry_button_system`] for boundary conditions, loads,
/// materials, and sections alike, so creating something by accident (a
/// near-certainty when iterating on a model) is always one click to undo.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) enum DeleteSetupEntry {
    BoundaryCondition(usize),
    LoadGroup(usize),
    DistributedLoad(usize),
    Material(usize),
    Section(usize),
}

/// Marks the container that [`rebuild_boundary_loads_list`] fills with one
/// removable entry per loaded [`fem_core::BoundaryCondition`] and load
/// group.
#[derive(Component)]
pub(crate) struct BoundaryLoadsListContainer;

/// Marks the container that [`rebuild_materials_sections_list`] fills with
/// one read-only entry per loaded [`fem_core::FemMaterial`] and
/// [`fem_core::Section`].
#[derive(Component)]
pub(crate) struct MaterialsSectionsListContainer;

/// Marks any scrollable list container (currently [`SetsListContainer`] and
/// [`MaterialsSectionsListContainer`]) so [`handle_scrollable_list_wheel`]
/// can find and scroll whichever one the cursor is over.
///
/// `Overflow::scroll_y()` alone only clips content visually; without this
/// system actually moving `ScrollPosition` in response to `MouseWheel`
/// events, a list taller than its container's `max_height` is unreachable —
/// the content exists but there's no way to see it.
#[derive(Component)]
pub(crate) struct ScrollableList;

/// Marks the main panel scroll area (the entire left-panel content below
/// the title bar). Handled by [`handle_panel_wheel`] — separate from the
/// smaller [`ScrollableList`] handler so the two don't fight each other
/// when the cursor is over both.
#[derive(Component)]
pub(crate) struct PanelScrollArea;

/// Marks a UI region that blocks pointer gestures from reaching the 3-D
/// viewport. This covers the whole sidebar, including non-button areas.
#[derive(Component)]
pub(crate) struct UiInputCapture;

/// The task currently shown in the left sidebar.
///
/// Keeping one task visible at a time prevents the sidebar from becoming a
/// single, ever-growing form as FrontISTR features are added.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SidebarPage {
    #[default]
    Model,
    Contact,
    Loads,
    Materials,
    Solve,
    Results,
}

#[derive(Component)]
pub(crate) struct SidebarPageButton {
    pub page: SidebarPage,
}

/// Visibility mask for a sidebar section or subsection.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SidebarPageContent(u8);

impl SidebarPageContent {
    const MODEL: u8 = 1 << 0;
    const CONTACT: u8 = 1 << 1;
    const LOADS: u8 = 1 << 2;
    const MATERIALS: u8 = 1 << 3;
    const SOLVE: u8 = 1 << 4;
    const RESULTS: u8 = 1 << 5;

    const fn page(page: SidebarPage) -> Self {
        Self(match page {
            SidebarPage::Model => Self::MODEL,
            SidebarPage::Contact => Self::CONTACT,
            SidebarPage::Loads => Self::LOADS,
            SidebarPage::Materials => Self::MATERIALS,
            SidebarPage::Solve => Self::SOLVE,
            SidebarPage::Results => Self::RESULTS,
        })
    }

    const fn analysis() -> Self {
        Self(Self::LOADS | Self::MATERIALS | Self::SOLVE)
    }

    const fn part_position() -> Self {
        Self(Self::MODEL | Self::CONTACT)
    }

    const fn contains(self, page: SidebarPage) -> bool {
        self.0 & Self::page(page).0 != 0
    }
}

/// Which kind of [`fem_core::FemMesh`] set a [`SetButton`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetKind {
    Node,
    Element,
    Surface,
}

/// References one set (by mesh + kind + index within that mesh's set list)
/// so [`set_button_system`] can resolve it back to a list of
/// [`fem_core::FemEntityId`]s to select when clicked.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SetButton {
    pub mesh_index: usize,
    pub kind: SetKind,
    pub set_index: usize,
}

/// Marks the container that [`rebuild_sets_list`] fills with one
/// [`SetButton`] per node/element/surface set in the loaded model.
#[derive(Component)]
pub(crate) struct SetsListContainer;

pub(crate) fn spawn_ui(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(12.0),
                top: px(12.0),
                width: px(320.0),
                // A definite height lets the flex child below shrink and
                // scroll instead of growing the panel beyond the viewport.
                height: Val::Vh(96.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(0.0),
                padding: UiRect::all(px(0.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(7.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(PANEL_BORDER),
            UiInputCapture,
            Name::new("FemUiPanel"),
        ))
        .with_children(|panel| {
            // ── Title bar (fixed / not scrolled) ────────────────────────
            panel.spawn((
                Node {
                    padding: UiRect::axes(px(12.0), px(8.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.08, 0.11, 0.14, 0.98)),
            )).with_children(|bar| {
                bar.spawn((
                    Text::new("bevyistr"),
                    TextFont { font_size: FontSize::Px(13.0), ..default() },
                    TextColor(Color::srgb(0.78, 0.88, 0.96)),
                ));
            });
            divider(panel);

            // Task navigation stays visible while the active page scrolls.
            spawn_sidebar_navigation(panel);
            spawn_view_toolbar(panel);
            divider(panel);
            spawn_selection_level_bar(panel);
            divider(panel);

            // ── Scrollable content area ──────────────────────────────────
            panel.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::scroll_y(),
                    flex_grow: 1.0,
                    ..default()
                },
                ScrollPosition::default(),
                PanelScrollArea,
                Name::new("PanelScrollArea"),
            ))
            .with_children(|panel| {

            sidebar_page_group(panel, SidebarPageContent::page(SidebarPage::Model), "ModelFilePage", |panel| {

            // ── § File ──────────────────────────────────────────────────
            section(panel, "FILE", |sec| {
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(8.0),
                        ..default()
                    },
                )).with_children(|row| {
                    row.spawn((
                        Button,
                        Node {
                            padding: UiRect::axes(px(14.0), px(5.0)),
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(5.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(BUTTON_NORMAL),
                        BorderColor::all(PANEL_BORDER),
                        OpenMeshButton,
                        Name::new("OpenMeshButton"),
                    )).with_child((
                        Text::new("Open Mesh"),
                        TextFont { font_size: FontSize::Px(12.0), ..default() },
                        TextColor(TEXT_MAIN),
                    ));

                    row.spawn((
                        Button,
                        Node {
                            padding: UiRect::axes(px(14.0), px(5.0)),
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(5.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(BUTTON_NORMAL),
                        BorderColor::all(PANEL_BORDER),
                        ImportMeshButton,
                        Name::new("ImportMeshButton"),
                    )).with_child((
                        Text::new("+ Import"),
                        TextFont { font_size: FontSize::Px(12.0), ..default() },
                        TextColor(TEXT_MAIN),
                    ));
                });
                sec.spawn((
                    Text::new("No mesh loaded"),
                    TextFont { font_size: FontSize::Px(11.5), ..default() },
                    TextColor(TEXT_MUTED),
                    MeshStatsText,
                ));
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(8.0),
                        ..default()
                    },
                )).with_children(|row| {
                    row.spawn((
                        Button,
                        Node {
                            flex_grow: 1.0,
                            padding: UiRect::axes(px(8.0), px(4.0)),
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(5.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.10, 0.30, 0.18)),
                        BorderColor::all(Color::srgb(0.15, 0.46, 0.26)),
                        OpenProjectButton,
                        Name::new("OpenProjectButton"),
                    )).with_child((
                        Text::new("Open Project (hecmw_ctrl.dat)"),
                        TextFont { font_size: FontSize::Px(10.5), ..default() },
                        TextColor(Color::srgb(0.75, 0.97, 0.80)),
                    ));
                });
                hint_text(sec, "Open Project = load .msh + .cnt together   Open Mesh / + Import = mesh only");
            });
            divider(panel);

            }); // end ModelFilePage

            sidebar_page_group(panel, SidebarPageContent::part_position(), "PartPositionPage", |panel| {

            // ── § Assembly ──────────────────────────────────────────────
            section(panel, "PART POSITION", |sec| {
                sec.spawn((
                    Button,
                    Node {
                        width: percent(100.0),
                        height: px(30.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(5.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(PANEL_BORDER),
                    AssemblyModeButton,
                    Name::new("AssemblyModeButton"),
                ))
                .with_child((
                    Text::new("Edit in viewport: OFF"),
                    TextFont { font_size: FontSize::Px(11.5), ..default() },
                    TextColor(TEXT_MAIN),
                    AssemblyModeButtonLabel,
                ));
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        ..default()
                    },
                ))
                .with_children(|row| {
                    for (index, mode) in AssemblyGizmoMode::ALL.into_iter().enumerate() {
                        let (radius, border) = segment_style(index == 0, index == 1);
                        row.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: px(27.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border,
                                border_radius: radius,
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            AssemblyGizmoModeButton { mode },
                            Name::new(format!("AssemblyGizmoMode_{}", mode.label())),
                        ))
                        .with_child((
                            Text::new(mode.label()),
                            TextFont {
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    }
                });
                hint_text(
                    sec,
                    "Move: drag X/Y/Z arrow   Rotate: drag RX/RY/RZ ring",
                );
                hint_text(sec, "Turn Edit OFF to return to node/face selection");
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4.0),
                        max_height: px(110.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                    ScrollableList,
                    AssemblyPartsContainer,
                    Name::new("AssemblyPartsContainer"),
                ));
                sec.spawn((
                    Text::new("No part selected"),
                    TextFont { font_size: FontSize::Px(10.5), ..default() },
                    TextColor(TEXT_MUTED),
                    AssemblyStatusText,
                ));

                spawn_slider(sec, SliderConfig {
                    width: 272.0,
                    min: 0.1,
                    max: 10.0,
                    value: 1.0,
                    label: "Move step (% of part size)",
                    id: SliderId::AssemblyMovePercent,
                });
                assembly_action_row(sec, [
                    ("-X", AssemblyTransformAction::Translate(-Vec3::X)),
                    ("-Y", AssemblyTransformAction::Translate(-Vec3::Y)),
                    ("-Z", AssemblyTransformAction::Translate(-Vec3::Z)),
                ]);
                assembly_action_row(sec, [
                    ("+X", AssemblyTransformAction::Translate(Vec3::X)),
                    ("+Y", AssemblyTransformAction::Translate(Vec3::Y)),
                    ("+Z", AssemblyTransformAction::Translate(Vec3::Z)),
                ]);

                spawn_slider(sec, SliderConfig {
                    width: 272.0,
                    min: 1.0,
                    max: 45.0,
                    value: 5.0,
                    label: "Rotate step (deg)",
                    id: SliderId::AssemblyRotationDegrees,
                });
                assembly_action_row(sec, [
                    ("-RX", AssemblyTransformAction::Rotate(-Vec3::X)),
                    ("-RY", AssemblyTransformAction::Rotate(-Vec3::Y)),
                    ("-RZ", AssemblyTransformAction::Rotate(-Vec3::Z)),
                ]);
                assembly_action_row(sec, [
                    ("+RX", AssemblyTransformAction::Rotate(Vec3::X)),
                    ("+RY", AssemblyTransformAction::Rotate(Vec3::Y)),
                    ("+RZ", AssemblyTransformAction::Rotate(Vec3::Z)),
                ]);

                action_button(
                    sec,
                    "Reset selected part pose",
                    AssemblyTransformButton { action: AssemblyTransformAction::Reset },
                    "AssemblyResetPoseButton",
                );
                hint_text(sec, "Real mesh coordinates are updated. Contact candidates are cleared after movement; run Detect again");
            });
            divider(panel);

            }); // end PartPositionPage

            sidebar_page_group(panel, SidebarPageContent::page(SidebarPage::Model), "ModelSelectionPage", |panel| {

            // ── § Selection ─────────────────────────────────────────────
            section(panel, "SELECTION", |sec| {
                sec.spawn((
                    Text::new("Filter: Element   Selected: 0   Hover: none"),
                    TextFont { font_size: FontSize::Px(11.5), ..default() },
                    TextColor(TEXT_MAIN),
                    SelectionStatsText,
                ));
                // Dynamic info: count + hover coords — updated every frame.
                sec.spawn((
                    Text::new("Selected: 0  |  Hover: -"),
                    TextFont { font_size: FontSize::Px(11.0), ..default() },
                    TextColor(Color::srgba(0.50, 0.78, 0.95, 0.90)),
                    SelectionInfoText,
                ));

                sec.spawn((
                    Node { flex_direction: FlexDirection::Row, column_gap: px(6.0), ..default() },
                )).with_children(|row| {
                    action_button(row, "Make Node Group",    MakeNodeGroupButton,    "MakeNodeGroupButton");
                    action_button(row, "Make Element Group", MakeElementGroupButton, "MakeElementGroupButton");
                });
                hint_text(sec, "Saves selection as NGRP/EGRP for use in BCs and sections");
            });
            divider(panel);

            }); // end ModelSelectionPage

            sidebar_page_group(panel, SidebarPageContent::page(SidebarPage::Model), "ModelSetsPage", |panel| {

            // ── § Sets / Groups ──────────────────────────────────────────
            section(panel, "SETS", |sec| {
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4.0),
                        max_height: px(120.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                    ScrollableList,
                    SetsListContainer,
                    Name::new("SetsListContainer"),
                ));
                hint_text(sec, "Click a set to select its members   Scroll to see more");
            });
            divider(panel);

            }); // end ModelSetsPage

            sidebar_page_group(panel, SidebarPageContent::page(SidebarPage::Contact), "ContactPage", |panel| {

            // ── § Contact definition ─────────────────────────────────────
            section(panel, "CONTACT DEFINITION", |sec| {
                sec.spawn((
                    Text::new("Defined contacts: 0"),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                    ContactDefinitionsText,
                ));
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4.0),
                        max_height: px(150.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                    ScrollableList,
                    ContactDefinitionsListContainer,
                    Name::new("ContactDefinitionsListContainer"),
                ));
                hint_text(sec, "Click a pair to review: master = blue, slave = orange");
                sec.spawn((
                    Text::new("NEW CONTACT — TOPOLOGY"),
                    TextFont {
                        font_size: FontSize::Px(9.2),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                ));
                sec.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    ..default()
                },))
                .with_children(|row| {
                    for (index, kind) in ContactPairKind::ALL.into_iter().enumerate() {
                        let (radius, border) = segment_style(index == 0, index == 1);
                        row.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: px(26.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border,
                                border_radius: radius,
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            ContactPairKindButton(kind),
                            Name::new(format!("ContactPairKind_{}", kind.label())),
                        ))
                        .with_child((
                            Text::new(kind.label()),
                            TextFont {
                                font_size: FontSize::Px(10.5),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    }
                });
                sec.spawn((
                    Text::new("BEHAVIOR"),
                    TextFont {
                        font_size: FontSize::Px(9.2),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                ));
                sec.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4.0),
                    ..default()
                },))
                .with_children(|row| {
                    for (contact_type, label) in [
                        (ContactType::SmallSliding, "Small"),
                        (ContactType::FiniteSliding, "Finite"),
                        (ContactType::Tied, "Tied"),
                    ] {
                        row.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: px(25.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            ContactBehaviorButton(contact_type),
                            Name::new(format!("ContactBehavior_{label}")),
                        ))
                        .with_child((
                            Text::new(label),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    }
                });
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(5.0),
                        ..default()
                    },
                    ContactSlidingParameterControls,
                    Name::new("ContactSlidingParameterControls"),
                ))
                .with_children(|parameters| {
                    spawn_slider(
                        parameters,
                        SliderConfig {
                            width: 272.0,
                            min: 0.0,
                            max: 1.0,
                            value: 0.0,
                            label: "Friction coefficient",
                            id: SliderId::ContactFriction,
                        },
                    );
                    action_button(
                        parameters,
                        "Edit friction exactly",
                        ContactParameterButton(ContactParameter::Friction),
                        "EditContactFrictionButton",
                    );
                    parameters
                        .spawn((
                            Button,
                            Node {
                                width: percent(100.0),
                                height: px(25.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            ContactPenaltyToggleButton,
                            Name::new("ContactPenaltyToggleButton"),
                        ))
                        .with_child((
                            Text::new("Penalty factor: AUTO"),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                            ContactPenaltyToggleLabel,
                        ));
                    parameters
                        .spawn((
                            Node {
                                display: Display::None,
                                flex_direction: FlexDirection::Column,
                                row_gap: px(5.0),
                                ..default()
                            },
                            ContactPenaltyControls,
                            Name::new("ContactPenaltyControls"),
                        ))
                        .with_children(|penalty| {
                            spawn_slider(
                                penalty,
                                SliderConfig {
                                    width: 272.0,
                                    min: 0.0,
                                    max: 1.0e6,
                                    value: 1.0e5,
                                    label: "Penalty factor",
                                    id: SliderId::ContactPenaltyFactor,
                                },
                            );
                            action_button(
                                penalty,
                                "Edit penalty exactly",
                                ContactParameterButton(ContactParameter::PenaltyFactor),
                                "EditContactPenaltyButton",
                            );
                        });
                });
                sec.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6.0),
                    ..default()
                },))
                .with_children(|row| {
                    action_button(
                        row,
                        "1  Capture Slave",
                        CaptureContactSideButton(ContactCaptureSide::Slave),
                        "CaptureContactSlaveButton",
                    );
                    action_button(
                        row,
                        "2  Capture Master",
                        CaptureContactSideButton(ContactCaptureSide::Master),
                        "CaptureContactMasterButton",
                    );
                });
                sec.spawn((
                    Text::new("Slave: not set   Master: not set"),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                    ContactDraftStatusText,
                ));
                action_button(
                    sec,
                    "3  Create Contact Pair",
                    FinalizeContactButton,
                    "FinalizeContactButton",
                );
                hint_text(sec, "Captured geometry is previewed immediately; master = blue, slave = orange");
            });
            divider(panel);

            // ── § Contact detection ──────────────────────────────────────
            section(panel, "CONTACT DETECTION", |sec| {
                action_button(
                    sec,
                    "Detect Contact Candidates",
                    DetectContactsButton,
                    "DetectContactsButton",
                );
                hint_text(sec, "Searches nearby opposing or coincident boundary faces");
            });
            divider(panel);

            // ── § Contact review ─────────────────────────────────────────
            section(panel, "CONTACT REVIEW", |sec| {
                sec.spawn((
                    Text::new("No candidates — run Detect Contact Candidates"),
                    TextFont { font_size: FontSize::Px(11.5), ..default() },
                    TextColor(TEXT_MUTED),
                    ContactCandidateText,
                ));
                sec.spawn((
                    Node {
                        display: Display::None,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(7.0),
                        ..default()
                    },
                    ContactReviewControls,
                    Name::new("ContactReviewControls"),
                )).with_children(|review| {
                    review.spawn((
                        Button,
                        Node {
                            width: percent(100.0),
                            height: px(28.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(5.0)),
                            ..default()
                        },
                        BackgroundColor(BUTTON_ACTIVE),
                        BorderColor::all(ACTIVE_BORDER),
                        ContactGhostToggleButton,
                        Name::new("ContactGhostToggleButton"),
                    )).with_child((
                        Text::new("Ghost others: ON"),
                        TextFont { font_size: FontSize::Px(10.5), ..default() },
                        TextColor(TEXT_MAIN),
                        ContactGhostToggleLabel,
                    ));
                    review.spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(6.0),
                            ..default()
                        },
                    )).with_children(|row| {
                        action_button(
                            row,
                            "Previous",
                            ContactCandidateActionButton(ContactCandidateAction::Previous),
                            "PreviousContactCandidateButton",
                        );
                        action_button(
                            row,
                            "Next",
                            ContactCandidateActionButton(ContactCandidateAction::Next),
                            "NextContactCandidateButton",
                        );
                    });
                    spawn_slider(review, SliderConfig {
                        width: 272.0,
                        min: 0.0,
                        max: 30.0,
                        value: 8.0,
                        label: "Review separation (% model size)",
                        id: SliderId::ContactReviewSeparation,
                    });
                    review.spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(6.0),
                            ..default()
                        },
                    )).with_children(|row| {
                        action_button(
                            row,
                            "Reject",
                            ContactCandidateActionButton(ContactCandidateAction::Reject),
                            "RejectContactCandidateButton",
                        );
                        action_button(row, "Accept", AcceptContactButton, "AcceptContactButton");
                    });
                });
                hint_text(
                    sec,
                    "Review display only — analysis coordinates and export are unchanged",
                );
            });
            divider(panel);

            }); // end ContactPage

            sidebar_page_group(panel, SidebarPageContent::analysis(), "AnalysisPages", |panel| {

            // ── § Analysis Setup (boundary conditions / loads / materials) ──
            section(panel, "ANALYSIS SETUP", |sec| {
                // Export row: Open Setup + Export to FrontISTR on the same row
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(6.0),
                        ..default()
                    },
                    SidebarPageContent::page(SidebarPage::Solve),
                )).with_children(|row| {
                    row.spawn((
                        Button,
                        Node {
                            flex_grow: 1.0,
                            height: px(28.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(5.0)),
                            ..default()
                        },
                        BackgroundColor(BUTTON_NORMAL),
                        BorderColor::all(PANEL_BORDER),
                        OpenSetupButton,
                        Name::new("OpenSetupButton"),
                    )).with_child((
                        Text::new("Open Setup"),
                        TextFont { font_size: FontSize::Px(11.0), ..default() },
                        TextColor(TEXT_MAIN),
                    ));

                    row.spawn((
                        Button,
                        Node {
                            flex_grow: 1.0,
                            height: px(28.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(5.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.10, 0.32, 0.18)),
                        BorderColor::all(Color::srgb(0.15, 0.50, 0.28)),
                        ExportButton,
                        Name::new("ExportButton"),
                    )).with_child((
                        Text::new("Export"),
                        TextFont { font_size: FontSize::Px(11.0), ..default() },
                        TextColor(Color::srgb(0.75, 0.97, 0.80)),
                    ));
                });
                sec.spawn((
                    Text::new(""),
                    TextFont { font_size: FontSize::Px(10.5), ..default() },
                    TextColor(TEXT_MUTED),
                    ExportStatusText,
                    SidebarPageContent::page(SidebarPage::Solve),
                ));
                sec.spawn((
                    Text::new("Setup: none loaded"),
                    TextFont { font_size: FontSize::Px(11.5), ..default() },
                    TextColor(TEXT_MUTED),
                    AnalysisSetupStatsText,
                    SidebarPageContent::page(SidebarPage::Solve),
                ));
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(6.0),
                        ..default()
                    },
                    SidebarPageContent::page(SidebarPage::Loads),
                )).with_children(|row| {
                    action_button(row, "Constraints", ToggleConstraintsButton, "ToggleConstraintsButton");
                    action_button(row, "Loads",       ToggleLoadsButton,       "ToggleLoadsButton");
                });
                page_hint_text(
                    sec,
                    SidebarPage::Loads,
                    "Red cone = fixed DOF    Orange arrow = nodal load",
                );

                // ── Create from current node selection ──────────────────
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(5.0),
                        margin: UiRect::top(px(6.0)),
                        padding: UiRect::all(px(6.0)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(5.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgba(0.30, 0.36, 0.40, 0.50)),
                    SidebarPageContent::page(SidebarPage::Loads),
                    Name::new("CreateFromSelectionPanel"),
                )).with_children(|panel| {
                    panel.spawn((
                        Text::new("Create from selection"),
                        TextFont { font_size: FontSize::Px(9.5), ..default() },
                        TextColor(Color::srgba(0.44, 0.60, 0.72, 0.90)),
                    ));

                    // Constraint presets
                    panel.spawn((
                        Node { flex_direction: FlexDirection::Row, column_gap: px(4.0), ..default() },
                    )).with_children(|row| {
                        constraint_preset_button(row, "Fix All", 1, 6);
                        constraint_preset_button(row, "Fix XYZ", 1, 3);
                        constraint_preset_button(row, "Fix X",   1, 1);
                        constraint_preset_button(row, "Fix Y",   2, 2);
                        constraint_preset_button(row, "Fix Z",   3, 3);
                    });

                    // Load direction + magnitude + apply
                    panel.spawn((
                        Node { flex_direction: FlexDirection::Row, column_gap: px(4.0), ..default() },
                    )).with_children(|row| {
                        load_direction_button(row, "+X", 1, 1.0);
                        load_direction_button(row, "-X", 1, -1.0);
                        load_direction_button(row, "+Y", 2, 1.0);
                        load_direction_button(row, "-Y", 2, -1.0);
                        load_direction_button(row, "+Z", 3, 1.0);
                        load_direction_button(row, "-Z", 3, -1.0);
                    });
                    spawn_slider(panel, SliderConfig {
                        width: 268.0,
                        min: 0.0,
                        max: 1000.0,
                        value: 100.0,
                        label: "Load magnitude",
                        id: SliderId::LoadMagnitude,
                    });
                    hint_text(
                        panel,
                        "Pick an axis to preview arrows; enter an exact value at lower right",
                    );
                    panel.spawn((
                        Button,
                        Node {
                            width: Val::Percent(100.0),
                            height: px(24.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(4.0)),
                            ..default()
                        },
                        BackgroundColor(BUTTON_NORMAL),
                        BorderColor::all(PANEL_BORDER),
                        ApplyLoadButton,
                        Name::new("ApplyLoadButton"),
                    )).with_child((
                        Text::new("Apply Load to Selection"),
                        TextFont { font_size: FontSize::Px(10.5), ..default() },
                        TextColor(TEXT_MAIN),
                        ApplyLoadLabel,
                    ));

                    // ── Distributed load (pressure / gravity) ───────────────
                    panel.spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(4.0),
                            margin: UiRect::top(px(6.0)),
                            padding: UiRect::all(px(6.0)),
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(5.0)),
                            ..default()
                        },
                        BorderColor::all(Color::srgba(0.40, 0.36, 0.20, 0.50)),
                        Name::new("DloadPanel"),
                    )).with_children(|dp| {
                        dp.spawn((
                            Text::new("Add Distributed Load (select faces)"),
                            TextFont { font_size: FontSize::Px(9.5), ..default() },
                            TextColor(Color::srgba(0.74, 0.68, 0.40, 0.90)),
                        ));

                        dp.spawn((
                            Node { flex_direction: FlexDirection::Row, column_gap: px(4.0), ..default() },
                        )).with_children(|row| {
                            for (kind, label) in [
                                (SelectedDloadKind::Pressure, "Pressure"),
                                (SelectedDloadKind::Gravity,  "Gravity"),
                            ] {
                                row.spawn((
                                    Button,
                                    Node {
                                        flex_grow: 1.0, height: px(22.0),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        border: UiRect::all(px(1.0)),
                                        border_radius: BorderRadius::all(px(4.0)),
                                        ..default()
                                    },
                                    BackgroundColor(BUTTON_NORMAL),
                                    BorderColor::all(PANEL_BORDER),
                                    DloadKindButton(kind),
                                    Name::new(format!("DloadKind_{label}")),
                                )).with_child((
                                    Text::new(label),
                                    TextFont { font_size: FontSize::Px(9.5), ..default() },
                                    TextColor(TEXT_MAIN),
                                ));
                            }
                        });

                        spawn_slider(dp, SliderConfig {
                            width: 268.0, min: 0.0, max: 100.0, value: 1.0,
                            label: "Pressure / Accel. magnitude",
                            id: SliderId::DloadMagnitude,
                        });

                        dp.spawn((
                            Button,
                            Node {
                                width: Val::Percent(100.0), height: px(24.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            ApplyDloadButton,
                            Name::new("ApplyDloadButton"),
                        )).with_child((
                            Text::new("Apply Distributed Load"),
                            TextFont { font_size: FontSize::Px(10.0), ..default() },
                            TextColor(TEXT_MAIN),
                            ApplyDloadLabel,
                        ));
                        hint_text(
                            dp,
                            "Pressure: select faces  Gravity: select elements  Apply commits",
                        );
                    });

                    // Clear All BCs & Loads — one click to undo everything
                    panel.spawn((
                        Button,
                        Node {
                            width: Val::Percent(100.0),
                            height: px(22.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(4.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.40, 0.12, 0.12, 0.80)),
                        BorderColor::all(Color::srgba(0.65, 0.20, 0.20, 0.80)),
                        ClearAllBcLoadsButton,
                        Name::new("ClearAllBcLoadsButton"),
                    )).with_child((
                        Text::new("Clear All BCs & Loads"),
                        TextFont { font_size: FontSize::Px(9.5), ..default() },
                        TextColor(Color::srgb(0.95, 0.80, 0.80)),
                    ));
                    hint_text(
                        panel,
                        "Constraints apply immediately; loads stay provisional until Apply",
                    );

                });

                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(5.0),
                        padding: UiRect::all(px(6.0)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(5.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgba(0.30, 0.50, 0.36, 0.50)),
                    SidebarPageContent::page(SidebarPage::Materials),
                    Name::new("MaterialsEditorPanel"),
                )).with_children(|panel| {

                    // Material presets
                    panel.spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(4.0),
                            margin: UiRect::top(px(4.0)),
                            ..default()
                        },
                    )).with_children(|row| {
                        for (index, preset) in material_presets().iter().enumerate() {
                            material_preset_button(row, index, preset.label);
                        }
                    });

                    // ── Section definition ────────────────────────────
                    panel.spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(4.0),
                            margin: UiRect::top(px(6.0)),
                            padding: UiRect::all(px(6.0)),
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(5.0)),
                            ..default()
                        },
                        BorderColor::all(Color::srgba(0.30, 0.50, 0.36, 0.50)),
                        Name::new("SectionDefPanel"),
                    )).with_children(|sp| {
                        sp.spawn((
                            Text::new("Add Section"),
                            TextFont { font_size: FontSize::Px(9.5), ..default() },
                            TextColor(Color::srgba(0.44, 0.70, 0.54, 0.90)),
                        ));

                        // Section type toggle
                        sp.spawn((
                            Node { flex_direction: FlexDirection::Row, column_gap: px(4.0), ..default() },
                        )).with_children(|row| {
                            for (kind, label) in [
                                (SelectedSectionType::Solid, "Solid"),
                                (SelectedSectionType::Shell, "Shell"),
                                (SelectedSectionType::Beam,  "Beam"),
                            ] {
                                row.spawn((
                                    Button,
                                    Node {
                                        flex_grow: 1.0, height: px(22.0),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        border: UiRect::all(px(1.0)),
                                        border_radius: BorderRadius::all(px(4.0)),
                                        ..default()
                                    },
                                    BackgroundColor(BUTTON_NORMAL),
                                    BorderColor::all(PANEL_BORDER),
                                    SectionTypeButton(kind),
                                    Name::new(format!("SectionType_{label}")),
                                )).with_child((
                                    Text::new(label),
                                    TextFont { font_size: FontSize::Px(9.5), ..default() },
                                    TextColor(TEXT_MAIN),
                                ));
                            }
                        });

                        // Thickness / area slider
                        spawn_slider(sp, SliderConfig {
                            width: 268.0, min: 0.0, max: 50.0, value: 2.0,
                            label: "Thickness / Area",
                            id: SliderId::SectionThickness,
                        });

                        // Dynamic EGRP button row
                        sp.spawn((
                            Node { flex_direction: FlexDirection::Row, column_gap: px(4.0), flex_wrap: FlexWrap::Wrap, row_gap: px(4.0), ..default() },
                            SectionDefEgrpRow,
                            Name::new("SectionDefEgrpRow"),
                        ));

                        // Dynamic material button row
                        sp.spawn((
                            Node { flex_direction: FlexDirection::Row, column_gap: px(4.0), flex_wrap: FlexWrap::Wrap, row_gap: px(4.0), ..default() },
                            SectionDefMatRow,
                            Name::new("SectionDefMatRow"),
                        ));

                        // Apply button
                        sp.spawn((
                            Button,
                            Node {
                                width: Val::Percent(100.0), height: px(24.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            AddSectionButton,
                            Name::new("AddSectionButton"),
                        )).with_child((
                            Text::new("Add Section"),
                            TextFont { font_size: FontSize::Px(10.5), ..default() },
                            TextColor(TEXT_MAIN),
                        ));
                    });
                });

                sec.spawn((
                    Text::new("Boundary conditions & loads:"),
                    TextFont { font_size: FontSize::Px(9.5), ..default() },
                    TextColor(Color::srgba(0.44, 0.60, 0.72, 0.90)),
                    Node { margin: UiRect::top(px(4.0)), ..default() },
                    SidebarPageContent::page(SidebarPage::Loads),
                ));
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(3.0),
                        max_height: px(90.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                    ScrollableList,
                    BoundaryLoadsListContainer,
                    SidebarPageContent::page(SidebarPage::Loads),
                    Name::new("BoundaryLoadsListContainer"),
                ));

                sec.spawn((
                    Text::new("Materials & sections:"),
                    TextFont { font_size: FontSize::Px(9.5), ..default() },
                    TextColor(Color::srgba(0.44, 0.60, 0.72, 0.90)),
                    Node { margin: UiRect::top(px(4.0)), ..default() },
                    SidebarPageContent::page(SidebarPage::Materials),
                ));
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(3.0),
                        max_height: px(90.0),
                        overflow: Overflow::scroll_y(),
                        margin: UiRect::top(px(4.0)),
                        ..default()
                    },
                    ScrollPosition::default(),
                    ScrollableList,
                    MaterialsSectionsListContainer,
                    SidebarPageContent::page(SidebarPage::Materials),
                    Name::new("MaterialsSectionsListContainer"),
                ));

                // ── Solver settings ──────────────────────────────────────
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4.0),
                        margin: UiRect::top(px(6.0)),
                        padding: UiRect::all(px(6.0)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(5.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgba(0.30, 0.36, 0.55, 0.50)),
                    SidebarPageContent::page(SidebarPage::Solve),
                    Name::new("SolverPanel"),
                )).with_children(|sp| {
                    sp.spawn((
                        Text::new("Solver Settings"),
                        TextFont { font_size: FontSize::Px(9.5), ..default() },
                        TextColor(Color::srgba(0.55, 0.65, 0.90, 0.90)),
                    ));

                    // Analysis type
                    sp.spawn((
                        Node { flex_direction: FlexDirection::Row, column_gap: px(4.0), ..default() },
                    )).with_children(|row| {
                        for t in [
                            fem_core::AnalysisType::Static,
                            fem_core::AnalysisType::NlStatic,
                            fem_core::AnalysisType::Dynamic,
                            fem_core::AnalysisType::Eigen,
                        ] {
                            row.spawn((
                                Button,
                                Node {
                                    flex_grow: 1.0, height: px(22.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    border: UiRect::all(px(1.0)),
                                    border_radius: BorderRadius::all(px(4.0)),
                                    ..default()
                                },
                                BackgroundColor(BUTTON_NORMAL),
                                BorderColor::all(PANEL_BORDER),
                                AnalysisTypeButton(t),
                                Name::new(format!("AnalysisType_{}", t.label())),
                            )).with_child((
                                Text::new(t.label()),
                                TextFont { font_size: FontSize::Px(9.0), ..default() },
                                TextColor(TEXT_MAIN),
                            ));
                        }
                    });

                    // Linear solver method
                    sp.spawn((
                        Node { flex_direction: FlexDirection::Row, column_gap: px(4.0), ..default() },
                    )).with_children(|row| {
                        for m in [
                            fem_core::LinearSolverMethod::Cg,
                            fem_core::LinearSolverMethod::Direct,
                            fem_core::LinearSolverMethod::Gmres,
                        ] {
                            row.spawn((
                                Button,
                                Node {
                                    flex_grow: 1.0, height: px(22.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    border: UiRect::all(px(1.0)),
                                    border_radius: BorderRadius::all(px(4.0)),
                                    ..default()
                                },
                                BackgroundColor(BUTTON_NORMAL),
                                BorderColor::all(PANEL_BORDER),
                                SolverMethodButton(m),
                                Name::new(format!("SolverMethod_{}", m.label())),
                            )).with_child((
                                Text::new(m.label()),
                                TextFont { font_size: FontSize::Px(9.0), ..default() },
                                TextColor(TEXT_MAIN),
                            ));
                        }
                    });
                    hint_text(sp, "Settings written to !SOLUTION / !SOLVER in .cnt");
                });
            });

            }); // end AnalysisPages

            // ── POST-PROCESS group ──────────────────────────────────────────
            sidebar_page_group(panel, SidebarPageContent::page(SidebarPage::Results), "ResultsPage", |panel| {

            // ── § Post-process ───────────────────────────────────────────
            section(panel, "POST-PROCESS", |sec| {
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        ..default()
                    },
                )).with_children(|row| {
                    row.spawn((
                        Button,
                        Node {
                            flex_grow: 1.0,
                            height: px(28.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(5.0)),
                            ..default()
                        },
                        BackgroundColor(BUTTON_NORMAL),
                        BorderColor::all(PANEL_BORDER),
                        OpenResultButton,
                        Name::new("OpenResultButton"),
                    )).with_child((
                        Text::new("Open Result  (.res / .vtu)"),
                        TextFont { font_size: FontSize::Px(12.0), ..default() },
                        TextColor(TEXT_MAIN),
                    ));
                });
                sec.spawn((
                    Text::new("Result: none loaded"),
                    TextFont { font_size: FontSize::Px(11.5), ..default() },
                    TextColor(TEXT_MUTED),
                    ResultStatsText,
                ));
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(8.0),
                        margin: UiRect::top(px(4.0)),
                        ..default()
                    },
                    Visibility::Hidden,
                    ResultSliderSection,
                    Name::new("ResultSliderSection"),
                )).with_children(|sliders| {
                    spawn_slider(sliders, SliderConfig {
                        width: 270.0,
                        min: 0.0,
                        max: 0.0,
                        value: 0.0,
                        label: "Step",
                        id: SliderId::ResultStep,
                    });
                    spawn_slider(sliders, SliderConfig {
                        width: 270.0,
                        min: 0.0,
                        max: 20.0,
                        value: 1.0,
                        label: "Deform scale",
                        id: SliderId::DeformScale,
                    });
                    hint_text(sliders, "Left / Right: step through results");

                    // ── Animation playback controls ──────────────────────
                    sliders.spawn((
                        Node { flex_direction: FlexDirection::Row, column_gap: px(4.0), margin: UiRect::top(px(6.0)), ..default() },
                    )).with_children(|row| {
                        // ◀◀
                        row.spawn((
                            Button,
                            Node { width: px(36.0), height: px(28.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(px(1.0)), border_radius: BorderRadius::all(px(5.0)), ..default() },
                            BackgroundColor(BUTTON_NORMAL), BorderColor::all(PANEL_BORDER),
                            PlaybackRewindButton,
                        )).with_child((Text::new("|<"), TextFont { font_size: FontSize::Px(10.0), ..default() }, TextColor(TEXT_MAIN)));

                        // ▶ / ‖
                        row.spawn((
                            Button,
                            Node { flex_grow: 1.0, height: px(28.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(px(1.0)), border_radius: BorderRadius::all(px(5.0)), ..default() },
                            BackgroundColor(BUTTON_NORMAL), BorderColor::all(PANEL_BORDER),
                            PlaybackPlayPauseButton,
                        )).with_child((
                            Text::new("Play"),
                            TextFont { font_size: FontSize::Px(11.0), ..default() },
                            TextColor(TEXT_MAIN),
                            PlaybackPlayPauseLabel,
                        ));

                        // ▶▶
                        row.spawn((
                            Button,
                            Node { width: px(36.0), height: px(28.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(px(1.0)), border_radius: BorderRadius::all(px(5.0)), ..default() },
                            BackgroundColor(BUTTON_NORMAL), BorderColor::all(PANEL_BORDER),
                            PlaybackEndButton,
                        )).with_child((Text::new(">|"), TextFont { font_size: FontSize::Px(10.0), ..default() }, TextColor(TEXT_MAIN)));
                    });

                    spawn_slider(sliders, SliderConfig {
                        width: 270.0, min: 0.5, max: 10.0, value: 2.0,
                        label: "Speed (steps/sec)",
                        id: SliderId::PlaybackSpeed,
                    });
                });
            });

            }); // end ResultsPage

            }); // end scrollable content area
        });
}

// ── Layout helpers ────────────────────────────────────────────────────────────

fn spawn_sidebar_navigation(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                padding: UiRect::axes(px(10.0), px(7.0)),
                ..default()
            },
            Name::new("SidebarNavigation"),
        ))
        .with_children(|nav| {
            nav.spawn((Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(4.0),
                ..default()
            },))
                .with_children(|row| {
                    sidebar_page_button(row, SidebarPage::Model, "Model");
                    sidebar_page_button(row, SidebarPage::Contact, "Contact");
                    sidebar_page_button(row, SidebarPage::Loads, "BC / Loads");
                });
            nav.spawn((Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(4.0),
                ..default()
            },))
                .with_children(|row| {
                    sidebar_page_button(row, SidebarPage::Materials, "Materials");
                    sidebar_page_button(row, SidebarPage::Solve, "Solve");
                    sidebar_page_button(row, SidebarPage::Results, "Results");
                });
        });
}

/// Persistent render controls shared by every workflow page. View style is
/// a viewport concern, not a Model-page setting, so it stays available while
/// defining contacts, loads, materials, solver settings, or reviewing results.
fn spawn_view_toolbar(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(3.0),
                padding: UiRect::axes(px(10.0), px(4.0)),
                ..default()
            },
            Name::new("ViewToolbar"),
        ))
        .with_children(|toolbar| {
            toolbar.spawn((
                Text::new("VIEW"),
                TextFont {
                    font_size: FontSize::Px(9.2),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
            toolbar
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    ..default()
                },))
                .with_children(|row| {
                    let count = VisualizationMode::ALL.len();
                    for (index, mode) in VisualizationMode::ALL.iter().enumerate() {
                        let (radius, border) = segment_style(index == 0, index + 1 == count);
                        row.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: px(26.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border,
                                border_radius: radius,
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            RenderModeButton { mode: *mode },
                            Name::new(format!("RenderMode_{}", mode.label())),
                        ))
                        .with_child((
                            Text::new(mode.label()),
                            TextFont {
                                font_size: FontSize::Px(10.5),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    }
                });
        });
}

fn spawn_selection_level_bar(parent: &mut ChildSpawnerCommands) {
    let levels = [
        (SelectionLevel::Node, "Node"),
        (SelectionLevel::Edge, "Edge"),
        (SelectionLevel::Face, "Face"),
        (SelectionLevel::Element, "Element"),
    ];

    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                padding: UiRect::axes(px(10.0), px(4.0)),
                ..default()
            },
            SelectionToolbar,
            Name::new("SelectionToolbar"),
        ))
        .with_children(|toolbar| {
            toolbar.spawn((
                Text::new("SELECT TARGET — MODEL"),
                TextFont {
                    font_size: FontSize::Px(9.2),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                SelectionContextText,
            ));
            toolbar
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    ..default()
                },))
                .with_children(|row| {
                    let count = levels.len();
                    for (index, (level, label)) in levels.iter().enumerate() {
                        let (radius, border) = segment_style(index == 0, index == count - 1);
                        row.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: px(26.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border,
                                border_radius: radius,
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            SelectionLevelButton { level: *level },
                            Name::new(format!("SelectionLevel_{label}")),
                        ))
                        .with_child((
                            Text::new(*label),
                            TextFont {
                                font_size: FontSize::Px(10.5),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    }
                });

            toolbar
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4.0),
                        width: percent(100.0),
                        ..default()
                    },
                    Visibility::Visible,
                    SurfaceSelectionControls,
                    Name::new("SurfaceSelectionControls"),
                ))
                .with_children(|surface| {
                    surface.spawn((
                        Text::new("SURFACE GROWTH — FACE / ELEMENT"),
                        TextFont {
                            font_size: FontSize::Px(9.2),
                            ..default()
                        },
                        TextColor(TEXT_MUTED),
                    ));
                    surface
                        .spawn((Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(4.0),
                            width: percent(100.0),
                            ..default()
                        },))
                        .with_children(|row| {
                            for mode in [
                                SurfaceSelectionMode::Single,
                                SurfaceSelectionMode::Coplanar,
                                SurfaceSelectionMode::Smooth,
                            ] {
                                row.spawn((
                                    Button,
                                    Node {
                                        flex_grow: 1.0,
                                        padding: UiRect::axes(px(6.0), px(4.0)),
                                        border: UiRect::all(px(1.0)),
                                        border_radius: BorderRadius::all(px(5.0)),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        ..default()
                                    },
                                    BackgroundColor(BUTTON_NORMAL),
                                    BorderColor::all(PANEL_BORDER),
                                    SurfaceSelectionModeButton { mode },
                                    Name::new(format!("SurfaceSelection_{}", mode.label())),
                                ))
                                .with_child((
                                    Text::new(mode.label()),
                                    TextFont {
                                        font_size: FontSize::Px(10.5),
                                        ..default()
                                    },
                                    TextColor(TEXT_MAIN),
                                ));
                            }
                        });
                    surface
                        .spawn((
                            Node {
                                flex_direction: FlexDirection::Column,
                                display: Display::None,
                                ..default()
                            },
                            SurfaceAngleControls,
                        ))
                        .with_children(|controls| {
                            spawn_slider(
                                controls,
                                SliderConfig {
                                    width: 272.0,
                                    min: 0.0,
                                    max: 90.0,
                                    value: DEFAULT_SMOOTH_ANGLE_DEG,
                                    label: "Smooth angle threshold (deg)",
                                    id: SliderId::SurfaceAngle,
                                },
                            );
                        });
                    surface.spawn((
                        Text::new(
                            "Element keeps volume targets; growth follows the visible surface",
                        ),
                        TextFont {
                            font_size: FontSize::Px(9.4),
                            ..default()
                        },
                        TextColor(Color::srgba(0.45, 0.54, 0.60, 0.80)),
                        SurfaceSelectionHint,
                    ));
                });

            toolbar.spawn((
                Text::new("Node / Edge use Single; double/triple click expands connectivity"),
                TextFont {
                    font_size: FontSize::Px(9.4),
                    ..default()
                },
                TextColor(Color::srgba(0.58, 0.70, 0.76, 0.88)),
                Node {
                    display: Display::None,
                    ..default()
                },
                SurfaceSelectionUnavailableHint,
            ));

            toolbar.spawn((
                Text::new("Action: REPLACE — click or drag starts a new selection"),
                TextFont {
                    font_size: FontSize::Px(10.2),
                    ..default()
                },
                TextColor(Color::srgba(0.50, 0.78, 0.95, 0.95)),
                SelectionOperationHint,
            ));
            spawn_selection_guide(toolbar);
        });
}

fn spawn_selection_guide(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Button,
            Node {
                width: percent(100.0),
                min_height: px(24.0),
                padding: UiRect::axes(px(8.0), px(3.0)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.14, 0.18, 0.96)),
            BorderColor::all(Color::srgba(0.30, 0.58, 0.72, 0.90)),
            SelectionGuideToggle,
            Name::new("SelectionGuideToggle"),
        ))
        .with_child((
            Text::new("Selection guide  [hide]"),
            TextFont {
                font_size: FontSize::Px(10.5),
                ..default()
            },
            TextColor(Color::srgb(0.72, 0.88, 0.96)),
        ));
    parent
        .spawn((
            Node {
                width: percent(100.0),
                padding: UiRect::all(px(7.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.045, 0.075, 0.095, 0.94)),
            BorderColor::all(Color::srgba(0.22, 0.40, 0.50, 0.78)),
            SelectionGuidePanel,
            Name::new("SelectionGuidePanel"),
        ))
        .with_child((
            Text::new(SELECTION_GUIDE_TEXT),
            TextFont {
                font_size: FontSize::Px(9.6),
                ..default()
            },
            TextColor(Color::srgba(0.70, 0.78, 0.82, 0.94)),
        ));
}

fn sidebar_page_button(parent: &mut ChildSpawnerCommands, page: SidebarPage, label: &'static str) {
    parent
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                height: px(26.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            SidebarPageButton { page },
            Name::new(format!("SidebarPage_{label}")),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(10.5),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));
}

fn sidebar_page_group(
    parent: &mut ChildSpawnerCommands,
    pages: SidebarPageContent,
    name: &'static str,
    children_fn: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                display: sidebar_page_display(pages, SidebarPage::Model),
                ..default()
            },
            pages,
            Name::new(name),
        ))
        .with_children(children_fn);
}

fn sidebar_page_display(pages: SidebarPageContent, page: SidebarPage) -> Display {
    if pages.contains(page) {
        Display::Flex
    } else {
        Display::None
    }
}

fn page_hint_text(parent: &mut ChildSpawnerCommands, page: SidebarPage, text: &'static str) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(Color::srgba(0.45, 0.54, 0.60, 0.80)),
        SidebarPageContent::page(page),
    ));
}

fn divider(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Node {
            width: Val::Percent(100.0),
            height: px(1.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.28, 0.34, 0.38, 0.60)),
    ));
}

fn section(
    parent: &mut ChildSpawnerCommands,
    title: &'static str,
    children_fn: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(7.0),
                padding: UiRect::axes(px(12.0), px(10.0)),
                ..default()
            },
            Name::new(format!("Section_{title}")),
        ))
        .with_children(|sec| {
            sec.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(Color::srgba(0.44, 0.60, 0.72, 0.90)),
            ));
            children_fn(sec);
        });
}

fn action_button(
    parent: &mut ChildSpawnerCommands,
    label: &'static str,
    marker: impl Bundle,
    name: &'static str,
) {
    parent
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                height: px(28.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(5.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            marker,
            Name::new(name),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(11.5),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));
}

fn assembly_action_row(
    parent: &mut ChildSpawnerCommands,
    actions: [(&'static str, AssemblyTransformAction); 3],
) {
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(5.0),
            ..default()
        },))
        .with_children(|row| {
            for (label, action) in actions {
                action_button(
                    row,
                    label,
                    AssemblyTransformButton { action },
                    "AssemblyTransformButton",
                );
            }
        });
}

fn constraint_preset_button(
    parent: &mut ChildSpawnerCommands,
    label: &'static str,
    dof_start: u8,
    dof_end: u8,
) {
    parent
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                height: px(24.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            ConstraintPresetButton {
                dof_start,
                dof_end,
                label,
            },
            Name::new(format!("ConstraintPreset_{label}")),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(9.5),
                ..default()
            },
            TextColor(TEXT_MAIN),
            ConstraintPresetLabel, // ← enables dynamic count label
        ));
}

fn load_direction_button(
    parent: &mut ChildSpawnerCommands,
    label: &'static str,
    dof: u8,
    sign: f32,
) {
    parent
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                height: px(24.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            LoadDirectionButton { dof, sign },
            Name::new(format!("LoadDirection_{label}")),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(9.5),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));
}

fn material_preset_button(
    parent: &mut ChildSpawnerCommands,
    preset_index: usize,
    label: &'static str,
) {
    parent
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                height: px(22.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            MaterialPresetButton { preset_index },
            Name::new(format!("MaterialPreset_{label}")),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(9.5),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));
}

/// One built-in material preset offered by the "Create from selection"
/// panel's material buttons. Values are representative engineering
/// constants in SI-ish units (Pa, kg/m³) — close enough for a starting
/// point; a person doing precision work will still want to verify/adjust
/// these against their actual material spec via the loaded `.cnt`.
struct MaterialPreset {
    label: &'static str,
    name: &'static str,
    young_modulus: f32,
    poisson_ratio: f32,
    density: f32,
}

fn material_presets() -> &'static [MaterialPreset] {
    const PRESETS: &[MaterialPreset] = &[
        MaterialPreset {
            label: "+ Steel",
            name: "STEEL",
            young_modulus: 2.05e11,
            poisson_ratio: 0.30,
            density: 7850.0,
        },
        MaterialPreset {
            label: "+ Aluminum",
            name: "ALUMINUM",
            young_modulus: 6.90e10,
            poisson_ratio: 0.33,
            density: 2700.0,
        },
        MaterialPreset {
            label: "+ Concrete",
            name: "CONCRETE",
            young_modulus: 3.00e10,
            poisson_ratio: 0.20,
            density: 2400.0,
        },
        MaterialPreset {
            label: "+ Titanium",
            name: "TITANIUM",
            young_modulus: 1.14e11,
            poisson_ratio: 0.34,
            density: 4500.0,
        },
    ];

    PRESETS
}

fn hint_text(parent: &mut ChildSpawnerCommands, text: &'static str) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(Color::srgba(0.45, 0.54, 0.60, 0.80)),
    ));
}

fn segment_style(is_first: bool, is_last: bool) -> (BorderRadius, UiRect) {
    let r = 5.0f32;
    let border = UiRect {
        top: px(1.0),
        bottom: px(1.0),
        left: if is_first { px(1.0) } else { px(0.0) },
        right: px(1.0),
    };
    let (tl, bl) = if is_first { (r, r) } else { (0.0, 0.0) };
    let (tr, br) = if is_last { (r, r) } else { (0.0, 0.0) };
    (BorderRadius::new(px(tl), px(tr), px(br), px(bl)), border)
}

/// Handles sidebar page selection, paints the active page button, and resets
/// the content scroll position whenever the task changes.
pub(crate) fn sidebar_page_button_system(
    mut page: ResMut<SidebarPage>,
    mut tool: ResMut<ViewportTool>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &SidebarPageButton,
        ),
        With<SidebarPageButton>,
    >,
    mut scroll_areas: Query<&mut ScrollPosition, With<PanelScrollArea>>,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let page_changed = *page != button.page;
            *page = button.page;
            if page_changed {
                measurement.clear();
            }
            if !page_supports_part_position(button.page) {
                *tool = ViewportTool::Selection;
            }
            for mut scroll in &mut scroll_areas {
                scroll.0.y = 0.0;
            }
        }

        let active = *page == button.page;

        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::None, false) => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

fn page_supports_part_position(page: SidebarPage) -> bool {
    matches!(page, SidebarPage::Model | SidebarPage::Contact)
}

/// Shows only content associated with the current sidebar task. Inactive
/// content uses `display: none` rather than [`Visibility::Hidden`]: hidden UI
/// nodes still participate in layout and would otherwise push the active
/// page below a large blank area. Nested masks are supported: the analysis
/// shell is displayed for Loads, Materials, and Solve while its children
/// select one of those pages.
pub(crate) fn update_sidebar_page_visibility(
    page: Res<SidebarPage>,
    mut content: Query<(&SidebarPageContent, &mut Node)>,
) {
    if !page.is_changed() {
        return;
    }

    for (pages, mut node) in &mut content {
        node.display = sidebar_page_display(*pages, *page);
    }
}

struct SelectionPageContext {
    label: &'static str,

    levels: &'static [SelectionLevel],

    preferred: SelectionLevel,
}

const MODEL_SELECTION_LEVELS: &[SelectionLevel] = &[
    SelectionLevel::Node,
    SelectionLevel::Edge,
    SelectionLevel::Face,
    SelectionLevel::Element,
];
const CONTACT_SELECTION_LEVELS: &[SelectionLevel] = &[
    SelectionLevel::Node,
    SelectionLevel::Face,
    SelectionLevel::Element,
];
const LOAD_SELECTION_LEVELS: &[SelectionLevel] = &[
    SelectionLevel::Node,
    SelectionLevel::Face,
    SelectionLevel::Element,
];
const MATERIAL_SELECTION_LEVELS: &[SelectionLevel] = &[SelectionLevel::Element];
const NO_SELECTION_LEVELS: &[SelectionLevel] = &[];

fn selection_context_for_page(page: SidebarPage) -> SelectionPageContext {
    match page {
        SidebarPage::Model => SelectionPageContext {
            label: "SELECT TARGET — MODEL",
            levels: MODEL_SELECTION_LEVELS,
            preferred: SelectionLevel::Element,
        },
        SidebarPage::Contact => SelectionPageContext {
            label: "SELECT CONTACT SIDE",
            levels: CONTACT_SELECTION_LEVELS,
            preferred: SelectionLevel::Node,
        },
        SidebarPage::Loads => SelectionPageContext {
            label: "SELECT BC / LOAD TARGET",
            levels: LOAD_SELECTION_LEVELS,
            preferred: SelectionLevel::Node,
        },
        SidebarPage::Materials => SelectionPageContext {
            label: "SELECT SECTION ELEMENTS",
            levels: MATERIAL_SELECTION_LEVELS,
            preferred: SelectionLevel::Element,
        },
        SidebarPage::Solve | SidebarPage::Results => SelectionPageContext {
            label: "",
            levels: NO_SELECTION_LEVELS,
            preferred: SelectionLevel::Element,
        },
    }
}

/// Adapts the shared selection toolbar to the active workflow instead of
/// presenting every topology target on every page.
pub(crate) fn update_selection_context(
    mut commands: Commands,
    page: Res<SidebarPage>,
    mut filter: ResMut<SelectionFilter>,
    mut hover: ResMut<HoverResult>,
    mut selection: ResMut<SelectionState>,
    mut toolbars: Query<&mut Node, (With<SelectionToolbar>, Without<SelectionLevelButton>)>,
    mut labels: Query<&mut Text, With<SelectionContextText>>,
    mut level_buttons: Query<(&SelectionLevelButton, &mut Node), Without<SelectionToolbar>>,
    hovered_query: Query<Entity, With<Hovered>>,
    selected_query: Query<Entity, With<Selected>>,
) {
    let page_changed = page.is_changed();
    let filter_changed = filter.is_changed();

    if !page_changed && !filter_changed {
        return;
    }

    let context = selection_context_for_page(*page);
    let has_targets = !context.levels.is_empty();

    for mut toolbar in &mut toolbars {
        toolbar.display = if has_targets {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut label in &mut labels {
        **label = context.label.to_string();
    }
    for (button, mut node) in &mut level_buttons {
        let Some(index) = context
            .levels
            .iter()
            .position(|level| *level == button.level)
        else {
            node.display = Display::None;
            continue;
        };

        node.display = Display::Flex;
        let (radius, border) = segment_style(index == 0, index + 1 == context.levels.len());
        node.border_radius = radius;
        node.border = border;
    }

    if !has_targets {
        return;
    }

    let current_is_allowed = context.levels.contains(&filter.level);
    let target_level = if current_is_allowed && (!page_changed || selection.len() > 0) {
        filter.level
    } else {
        context.preferred
    };

    if target_level == filter.level {
        return;
    }

    filter.level = target_level;
    hover.clear();
    for entity in &hovered_query {
        commands.entity(entity).remove::<Hovered>();
    }

    if !current_is_allowed {
        selection.clear();
        for entity in &selected_query {
            commands.entity(entity).remove::<Selected>();
        }
    }
}

/// Scrolls whichever [`ScrollableList`] container the cursor is over in
/// response to `MouseWheel` events.
///
/// `Overflow::scroll_y()` only clips content visually — Bevy does not wire
/// up mouse-wheel input to `ScrollPosition` automatically outside of its
/// picking-based UI examples, which this project doesn't otherwise use, so
/// this reimplements the cursor-hit-test approach already used by
/// [`crate::slider::update_sliders`] for consistency with the rest of the
/// custom `bevy_ui` widgets here — including the same physical→logical
/// pixel conversion via `ComputedNode::inverse_scale_factor`; see that
/// function's doc comment for why both `UiGlobalTransform` (not the
/// 3D-world `GlobalTransform`, which `Node` entities don't carry) and that
/// scale factor are needed.
pub(crate) fn handle_scrollable_list_wheel(
    mut wheel_events: MessageReader<MouseWheel>,
    windows: Query<&Window>,
    mut scrollable_query: Query<
        (&mut ScrollPosition, &ComputedNode, &UiGlobalTransform),
        With<ScrollableList>,
    >,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    for ev in wheel_events.read() {
        let line_height = 24.0;
        let delta_y = match ev.unit {
            MouseScrollUnit::Line => ev.y * line_height,
            MouseScrollUnit::Pixel => ev.y,
        };

        if delta_y == 0.0 {
            continue;
        }

        for (mut scroll, node, transform) in &mut scrollable_query {
            let scale = node.inverse_scale_factor;
            let size = node.size() * scale;
            let origin = transform.transform_point2(Vec2::ZERO) * scale - size * 0.5;

            let over = cursor.x >= origin.x
                && cursor.x <= origin.x + size.x
                && cursor.y >= origin.y
                && cursor.y <= origin.y + size.y;

            if over {
                scroll.0.y = (scroll.0.y - delta_y).max(0.0);
            }
        }
    }
}

/// Scrolls the main left-panel content area with the mouse wheel, but
/// only when the cursor is NOT over a [`ScrollableList`] widget (those
/// consume the scroll event themselves in [`handle_scrollable_list_wheel`]).
///
/// See [`crate::slider::update_sliders`]'s doc comment for why this uses
/// `UiGlobalTransform` + `ComputedNode::inverse_scale_factor` rather than
/// `GlobalTransform` directly.
pub(crate) fn handle_panel_wheel(
    mut wheel_events: MessageReader<MouseWheel>,
    windows: Query<&Window>,
    mut panel_query: Query<
        (&mut ScrollPosition, &ComputedNode, &UiGlobalTransform),
        With<PanelScrollArea>,
    >,
    list_query: Query<(&ComputedNode, &UiGlobalTransform), With<ScrollableList>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    // If cursor is over any ScrollableList, the list handler takes priority.
    let over_sublist = list_query.iter().any(|(node, transform)| {
        let scale = node.inverse_scale_factor;
        let size = node.size() * scale;
        let origin = transform.transform_point2(Vec2::ZERO) * scale - size * 0.5;
        cursor.x >= origin.x
            && cursor.x <= origin.x + size.x
            && cursor.y >= origin.y
            && cursor.y <= origin.y + size.y
    });

    if over_sublist {
        return;
    }

    for ev in wheel_events.read() {
        let line_height = 28.0;
        let delta_y = match ev.unit {
            MouseScrollUnit::Line => ev.y * line_height,
            MouseScrollUnit::Pixel => ev.y,
        };

        if delta_y == 0.0 {
            continue;
        }

        for (mut scroll, node, transform) in &mut panel_query {
            let scale = node.inverse_scale_factor;
            let size = node.size() * scale;
            let origin = transform.transform_point2(Vec2::ZERO) * scale - size * 0.5;

            let over_panel = cursor.x >= origin.x
                && cursor.x <= origin.x + size.x
                && cursor.y >= origin.y
                && cursor.y <= origin.y + size.y;

            if over_panel {
                scroll.0.y = (scroll.0.y - delta_y).max(0.0);
            }
        }
    }
}

pub(crate) fn update_ui_pointer_state(
    mut state: ResMut<UiPointerState>,
    windows: Query<&Window>,
    capture_regions: Query<(&ComputedNode, &UiGlobalTransform), With<UiInputCapture>>,
    interactions: Query<&Interaction, With<Button>>,
) {
    let over_capture_region = windows
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .is_some_and(|cursor| {
            capture_regions.iter().any(|(node, transform)| {
                let scale = node.inverse_scale_factor;
                let size = node.size() * scale;
                let origin = transform.transform_point2(Vec2::ZERO) * scale - size * 0.5;

                cursor.x >= origin.x
                    && cursor.x <= origin.x + size.x
                    && cursor.y >= origin.y
                    && cursor.y <= origin.y + size.y
            })
        });

    let over_interactive_widget = interactions
        .iter()
        .any(|interaction| *interaction != Interaction::None);

    state.over_ui = over_capture_region || over_interactive_widget;
}

pub(crate) fn selection_guide_toggle_system(
    mut state: ResMut<SelectionGuideState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &Children,
        ),
        With<SelectionGuideToggle>,
    >,
    mut panels: Query<&mut Node, With<SelectionGuidePanel>>,
    mut labels: Query<&mut Text>,
) {
    for (interaction, mut background, mut border, children) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            state.expanded = !state.expanded;
        }

        *background = BackgroundColor(match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => Color::srgba(0.11, 0.20, 0.25, 0.98),
            Interaction::None => Color::srgba(0.08, 0.14, 0.18, 0.96),
        });
        *border = BorderColor::all(if state.expanded {
            ACTIVE_BORDER
        } else {
            Color::srgba(0.30, 0.58, 0.72, 0.90)
        });

        for &child in children {
            if let Ok(mut text) = labels.get_mut(child) {
                **text = if state.expanded {
                    "Selection guide  [hide]"
                } else {
                    "Selection guide  [show]"
                }
                .to_string();
            }
        }
    }

    for mut node in &mut panels {
        node.display = if state.expanded {
            Display::Flex
        } else {
            Display::None
        };
    }
}

pub(crate) fn update_selection_operation_hint(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut hints: Query<(&mut Text, &mut TextColor), With<SelectionOperationHint>>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
    let operation = SelectionOperation::from_modifiers(ctrl, shift, alt);
    let (label, color) = selection_operation_hint(operation);

    for (mut text, mut text_color) in &mut hints {
        if text.as_str() != label {
            **text = label.to_string();
        }
        if text_color.0 != color {
            text_color.0 = color;
        }
    }
}

fn selection_operation_hint(operation: SelectionOperation) -> (&'static str, Color) {
    match operation {
        SelectionOperation::Replace => (
            "Action: REPLACE — click or drag starts a new selection",
            Color::srgba(0.50, 0.78, 0.95, 0.95),
        ),
        SelectionOperation::Add => (
            "Action: ADD — Ctrl keeps the current selection",
            Color::srgba(0.42, 0.90, 0.60, 0.96),
        ),
        SelectionOperation::Toggle => (
            "Action: TOGGLE — Shift reverses selected / unselected",
            Color::srgba(0.98, 0.76, 0.34, 0.96),
        ),
        SelectionOperation::Remove => (
            "Action: REMOVE — Alt or Ctrl+Shift subtracts",
            Color::srgba(1.0, 0.48, 0.42, 0.96),
        ),
    }
}

pub(crate) fn selection_level_button_system(
    mut commands: Commands,
    mut filter: ResMut<SelectionFilter>,
    mut hover: ResMut<HoverResult>,
    mut selection: ResMut<SelectionState>,
    mut buttons: Query<
        (
            Entity,
            &Interaction,
            &SelectionLevelButton,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut Button,
        ),
        Without<RenderModeButton>,
    >,
    hovered_query: Query<Entity, With<Hovered>>,
    selected_query: Query<Entity, With<Selected>>,
) {
    for (_entity, interaction, button, mut background, mut border, mut bevy_button) in &mut buttons
    {
        if *interaction == Interaction::Pressed && filter.level != button.level {
            filter.level = button.level;
            hover.clear();
            selection.clear();

            for hovered in hovered_query.iter() {
                commands.entity(hovered).remove::<Hovered>();
            }

            for selected in selected_query.iter() {
                commands.entity(selected).remove::<Selected>();
            }
        }

        let active = filter.level == button.level;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::None, false) => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = if active {
            BorderColor::all(ACTIVE_BORDER)
        } else {
            BorderColor::all(PANEL_BORDER)
        };

        bevy_button.set_changed();
    }
}

pub(crate) fn render_mode_button_system(
    mut settings: ResMut<VisualizationSettings>,
    mut buttons: Query<
        (
            &Interaction,
            &RenderModeButton,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut Button,
        ),
        Without<SelectionLevelButton>,
    >,
) {
    for (interaction, button, mut background, mut border, mut bevy_button) in &mut buttons {
        if *interaction == Interaction::Pressed && settings.mode != button.mode {
            settings.mode = button.mode;
        }

        let active = settings.mode == button.mode;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::None, false) => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = if active {
            BorderColor::all(ACTIVE_BORDER)
        } else {
            BorderColor::all(PANEL_BORDER)
        };

        bevy_button.set_changed();
    }
}

pub(crate) fn open_mesh_button_system(
    mut request: ResMut<MeshLoadRequest>,
    mut status: ResMut<MeshLoadStatus>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<OpenMeshButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Open mesh")
                .add_filter("All supported meshes", &["msh", "geo", "inp"])
                .add_filter("HECMW / FrontISTR (.msh)", &["msh"])
                .add_filter("Gmsh geometry (.geo)", &["geo"])
                .add_filter("Abaqus / CalculiX (.inp)", &["inp"])
                .pick_file()
            {
                status.loading(path.clone());
                request.request(path);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Opens a mesh file and adds it as a new [`fem_core::Part`] alongside the
/// existing model, via [`MeshLoadRequest::request_import`], instead of
/// replacing the whole model like [`open_mesh_button_system`] does.
///
/// This is the entry point for building a mixed-part assembly (e.g. a shell
/// body plus a separately meshed bracket) by importing files one at a time.
pub(crate) fn import_mesh_button_system(
    mut request: ResMut<MeshLoadRequest>,
    mut status: ResMut<MeshLoadStatus>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ImportMeshButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Import mesh as new part")
                .add_filter("All supported meshes", &["msh", "geo", "inp"])
                .add_filter("HECMW / FrontISTR (.msh)", &["msh"])
                .add_filter("Gmsh geometry (.geo)", &["geo"])
                .add_filter("Abaqus / CalculiX (.inp)", &["inp"])
                .pick_file()
            {
                status.loading(path.clone());
                request.request_import(path);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Loads a mesh file, dispatching by extension:
///
/// * `.geo`  → Gmsh CLI then MSH v4.1 parser
/// * `.inp`  → Abaqus / CalculiX INP parser
/// * `.msh`  → HECMW first, falls back to Gmsh MSH v4.1
/// Loads a mesh file, dispatching by extension:
///
/// * `.geo`  → Gmsh CLI then MSH v4.1 parser
/// * `.inp`  → Abaqus / CalculiX INP parser
/// * `.msh`  → HECMW first, falls back to Gmsh MSH v4.1
///
/// When `request.import` is `true` (see [`MeshLoadRequest::request_import`]),
/// the mesh is added as a new [`fem_core::Part`] via [`FemModel::add_mesh`]
/// instead of replacing the whole model, building up a mixed-part assembly.
pub(crate) fn mesh_load_system(
    mut model: ResMut<FemModel>,
    mut request: ResMut<MeshLoadRequest>,
    mut status: ResMut<MeshLoadStatus>,
    mut version: ResMut<FemModelVersion>,
    mut camera_fit: ResMut<CameraFitRequest>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
) {
    let Some((path, import)) = request.take() else {
        return;
    };

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "geo" => match gmsh::run_gmsh(&path, None) {
            Ok(mesh) => {
                apply_mesh(
                    mesh,
                    &path,
                    import,
                    &mut model,
                    &mut status,
                    &mut version,
                    &mut camera_fit,
                    &mut setup,
                );
            }
            Err(e) => {
                status.failed(path, e.to_string());
            }
        },
        "inp" => match hecmw::load_inp_file(&path) {
            Ok(mesh) => {
                apply_mesh(
                    mesh,
                    &path,
                    import,
                    &mut model,
                    &mut status,
                    &mut version,
                    &mut camera_fit,
                    &mut setup,
                );
            }
            Err(e) => {
                status.failed(path, e.to_string());
            }
        },
        _ => {
            // .msh: HECMW project loader captures assignments and contact
            // pairs, then falls back to Gmsh when this is not a HEC-MW file.
            match hecmw::load_mesh_file_with_setup_and_contacts(&path) {
                Ok((mesh, materials, sections, contact_pairs)) => {
                    let mesh_index = if import { model.meshes.len() } else { 0 };
                    apply_mesh(
                        mesh,
                        &path,
                        import,
                        &mut model,
                        &mut status,
                        &mut version,
                        &mut camera_fit,
                        &mut setup,
                    );
                    let loaded_contacts =
                        merge_mesh_contact_pairs(&mut model, mesh_index, contact_pairs);
                    if loaded_contacts > 0 {
                        bevy::log::info!(
                            "Loaded {loaded_contacts} contact pair(s) from {:?}",
                            path.file_name()
                        );
                    }
                    // Merge embedded material/section blocks into AnalysisSetup,
                    // skipping duplicates by name.
                    let mut changed = false;
                    for m in materials {
                        if setup.material_by_name(&m.name).is_none() {
                            setup.materials.push(m);
                            changed = true;
                        }
                    }
                    for s in sections {
                        setup.sections.push(s);
                        changed = true;
                    }
                    if changed {
                        setup.set_changed();
                    }
                }
                Err(_) => match gmsh::load_msh_file(&path) {
                    Ok(mesh) => {
                        apply_mesh(
                            mesh,
                            &path,
                            import,
                            &mut model,
                            &mut status,
                            &mut version,
                            &mut camera_fit,
                            &mut setup,
                        );
                    }
                    Err(e) => {
                        status.failed(path, e.to_string());
                    }
                },
            }
        }
    }
}

fn merge_mesh_contact_pairs(
    model: &mut FemModel,
    mesh_index: usize,
    definitions: Vec<hecmw::HecmwContactPairDefinition>,
) -> usize {
    let Some(mesh) = model.meshes.get(mesh_index) else {
        return 0;
    };
    let mut resolved = Vec::new();

    for definition in definitions {
        let master = mesh.surface_sets.iter().position(|set| {
            set.name
                .eq_ignore_ascii_case(&definition.master_surface_name)
        });
        let Some(master) = master else {
            bevy::log::warn!(
                "Contact pair '{}' refers to missing master surface '{}'",
                definition.name,
                definition.master_surface_name
            );
            continue;
        };

        let master = fem_core::SurfaceSetRef::new(mesh_index, master);
        let contact =
            match definition.pair_type {
                hecmw::HecmwContactPairType::NodeSurface => {
                    let Some(slave) = mesh.node_sets.iter().position(|set| {
                        set.name.eq_ignore_ascii_case(&definition.slave_group_name)
                    }) else {
                        bevy::log::warn!(
                            "Contact pair '{}' refers to missing slave node group '{}'",
                            definition.name,
                            definition.slave_group_name
                        );
                        continue;
                    };
                    fem_core::ContactPair::new_node_surface(
                        definition.name,
                        master,
                        fem_core::NodeSetRef::new(mesh_index, slave),
                        ContactType::SmallSliding,
                    )
                }
                hecmw::HecmwContactPairType::SurfaceSurface => {
                    let Some(slave) = mesh.surface_sets.iter().position(|set| {
                        set.name.eq_ignore_ascii_case(&definition.slave_group_name)
                    }) else {
                        bevy::log::warn!(
                            "Contact pair '{}' refers to missing slave surface '{}'",
                            definition.name,
                            definition.slave_group_name
                        );
                        continue;
                    };
                    fem_core::ContactPair::new(
                        definition.name,
                        master,
                        fem_core::SurfaceSetRef::new(mesh_index, slave),
                        ContactType::SmallSliding,
                    )
                }
            };
        resolved.push(contact);
    }

    let count = resolved.len();
    model.contacts.extend(resolved);
    count
}

fn apply_mesh(
    mesh: fem_core::FemMesh,
    path: &std::path::PathBuf,
    import: bool,
    model: &mut FemModel,
    status: &mut MeshLoadStatus,
    version: &mut FemModelVersion,
    camera_fit: &mut CameraFitRequest,
    setup: &mut fem_core::AnalysisSetup,
) {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("mesh")
        .to_string();
    if import {
        model.add_mesh(name, mesh);
    } else {
        // Clear model-scoped setup at the same point the model is replaced.
        // Keeping this in the load transaction prevents a later rendering
        // system from erasing a .cnt file that was just applied by
        // `apply_pending_cnt_system`.
        setup.clear();
        *model = FemModel::single_mesh(name, mesh);
    }
    status.loaded(path.clone());
    version.bump();
    camera_fit.request();
}

/// Recenters and re-scales the orbit camera after a mesh file is loaded.
/// Assembly transforms intentionally do not request a fit, preserving the
/// view while a part is nudged repeatedly.
///
/// The first invocation (at startup) is skipped, since the app's startup
/// `setup` system already places the camera for the initial model.
pub(crate) fn camera_refit_on_reload(
    model: Option<Res<FemModel>>,
    request: Res<CameraFitRequest>,
    mut last_version: Local<Option<u64>>,
    mut camera_query: Query<(&mut Transform, &mut OrbitCamera)>,
) {
    let current = request.revision;

    if *last_version == Some(current) {
        return;
    }

    let first_run = last_version.is_none();
    *last_version = Some(current);

    if first_run {
        return;
    }

    let Some((min, max)) = model.as_deref().and_then(FemModel::bounds) else {
        return;
    };

    let (focus, radius) = camera::fit_bounds(min, max);
    let (min_radius, max_radius) = camera::radius_limits(radius);

    let Ok((mut transform, mut orbit)) = camera_query.single_mut() else {
        return;
    };

    orbit.focus = focus;
    orbit.target_focus = focus;
    orbit.radius = radius;
    orbit.min_radius = min_radius;
    orbit.max_radius = max_radius;

    let camera_position = focus + Vec3::new(radius * 0.45, radius * 0.45, radius);
    *transform = Transform::from_translation(camera_position).looking_at(focus, Vec3::Y);
}

pub(crate) fn create_surface_button_system(
    mut model: Option<ResMut<FemModel>>,
    selection: Res<SelectionState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<CreateSurfaceButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(model) = model.as_deref_mut() {
                let surface_set_count: usize = model
                    .meshes
                    .iter()
                    .map(|mesh| mesh.surface_sets.len())
                    .sum();
                let name = format!("SURFACE_{}", surface_set_count + 1);

                model.create_surface_set_from_targets(name, &selection.targets);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn create_contact_button_system(
    mut model: Option<ResMut<FemModel>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<CreateContactButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(model) = model.as_deref_mut() {
                let name = format!("CONTACT_{}", model.contacts.len() + 1);

                model.create_contact_pair_from_recent_surface_sets(name, ContactType::Tied);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn contact_pair_kind_button_system(
    mut commands: Commands,
    mut settings: ResMut<ContactDefinitionSettings>,
    mut draft: ResMut<ContactDraftPreview>,
    mut filter: ResMut<SelectionFilter>,
    mut selection: ResMut<SelectionState>,
    mut candidates: ResMut<ContactCandidateState>,
    selected_query: Query<Entity, With<Selected>>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &ContactPairKindButton,
        ),
        With<ContactPairKindButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed
            && interaction.is_changed()
            && settings.pair_kind != button.0
        {
            settings.pair_kind = button.0;
            draft.slave = None;
            settings.message = match button.0 {
                ContactPairKind::NodeSurface => {
                    filter.level = SelectionLevel::Node;
                    "Select slave nodes, then capture Slave".to_string()
                }
                ContactPairKind::SurfaceSurface => {
                    filter.level = SelectionLevel::Face;
                    "Select the slave surface, then capture Slave".to_string()
                }
            };
            candidates.candidates.clear();
            candidates.selected = None;
            selection.clear();
            for entity in &selected_query {
                commands.entity(entity).remove::<Selected>();
            }
        }

        let active = settings.pair_kind == button.0;
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });
    }
}

pub(crate) fn contact_behavior_button_system(
    mut settings: ResMut<ContactDefinitionSettings>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &ContactBehaviorButton,
        ),
        With<ContactBehaviorButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            settings.contact_type = button.0;
        }

        let active = settings.contact_type == button.0;
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });
    }
}

pub(crate) fn contact_penalty_toggle_button_system(
    mut settings: ResMut<ContactDefinitionSettings>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ContactPenaltyToggleButton>,
    >,
    mut labels: Query<&mut Text, With<ContactPenaltyToggleLabel>>,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            settings.use_penalty_factor = !settings.use_penalty_factor;
            settings.active_parameter = if settings.use_penalty_factor {
                ContactParameter::PenaltyFactor
            } else {
                ContactParameter::Friction
            };
            let (slider_id, label, units, value) = match settings.active_parameter {
                ContactParameter::Friction => (
                    SliderId::ContactFriction,
                    "Friction coefficient",
                    "dimensionless",
                    slider_value(&sliders, SliderId::ContactFriction, 0.0),
                ),
                ContactParameter::PenaltyFactor => (
                    SliderId::ContactPenaltyFactor,
                    "Contact penalty factor",
                    "FrontISTR input value",
                    slider_value(&sliders, SliderId::ContactPenaltyFactor, 1.0e5),
                ),
            };
            measurement.begin_slider_value(slider_id, label, units, value);
        }

        let active = settings.use_penalty_factor;
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });
    }

    for mut label in &mut labels {
        **label = format!(
            "Penalty factor: {}",
            if settings.use_penalty_factor {
                "CUSTOM"
            } else {
                "AUTO"
            }
        );
    }
}

pub(crate) fn contact_parameter_button_system(
    mut settings: ResMut<ContactDefinitionSettings>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &ContactParameterButton,
        ),
        With<ContactParameterButton>,
    >,
) {
    for (interaction, mut background, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            settings.active_parameter = button.0;
            let (slider_id, label, units, fallback) = match button.0 {
                ContactParameter::Friction => (
                    SliderId::ContactFriction,
                    "Friction coefficient",
                    "dimensionless",
                    0.0,
                ),
                ContactParameter::PenaltyFactor => (
                    SliderId::ContactPenaltyFactor,
                    "Contact penalty factor",
                    "FrontISTR input value",
                    1.0e5,
                ),
            };
            measurement.begin_slider_value(
                slider_id,
                label,
                units,
                slider_value(&sliders, slider_id, fallback),
            );
        }

        let active = settings.active_parameter == button.0;
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
    }
}

pub(crate) fn update_contact_parameter_controls(
    settings: Res<ContactDefinitionSettings>,
    mut sliding: Query<
        &mut Node,
        (
            With<ContactSlidingParameterControls>,
            Without<ContactPenaltyControls>,
        ),
    >,
    mut penalty: Query<
        &mut Node,
        (
            With<ContactPenaltyControls>,
            Without<ContactSlidingParameterControls>,
        ),
    >,
) {
    if !settings.is_changed() {
        return;
    }
    let sliding_visible = settings.contact_type != ContactType::Tied;
    for mut node in &mut sliding {
        node.display = if sliding_visible {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut penalty {
        node.display = if sliding_visible && settings.use_penalty_factor {
            Display::Flex
        } else {
            Display::None
        };
    }
}

pub(crate) fn sync_contact_measurement_box(
    page: Res<SidebarPage>,
    tool: Res<ViewportTool>,
    settings: Res<ContactDefinitionSettings>,
    sliders: Query<Ref<SliderState>, With<SliderTrack>>,
    mut measurement: ResMut<MeasurementBoxState>,
) {
    if *page != SidebarPage::Contact || *tool != ViewportTool::Selection {
        return;
    }
    if settings.contact_type == ContactType::Tied {
        if matches!(
            measurement.target,
            Some(MeasurementTarget::SliderValue {
                slider_id: SliderId::ContactFriction | SliderId::ContactPenaltyFactor,
                ..
            })
        ) {
            measurement.clear();
        }
        return;
    }

    let parameter = if settings.active_parameter == ContactParameter::PenaltyFactor
        && !settings.use_penalty_factor
    {
        ContactParameter::Friction
    } else {
        settings.active_parameter
    };
    let (slider_id, label, units, fallback) = match parameter {
        ContactParameter::Friction => (
            SliderId::ContactFriction,
            "Friction coefficient",
            "dimensionless",
            0.0,
        ),
        ContactParameter::PenaltyFactor => (
            SliderId::ContactPenaltyFactor,
            "Contact penalty factor",
            "FrontISTR input value",
            1.0e5,
        ),
    };
    let slider = sliders.iter().find(|slider| slider.id == slider_id);
    let value = slider
        .as_ref()
        .map(|slider| slider.value)
        .unwrap_or(fallback);
    let target_matches = matches!(
        measurement.target,
        Some(MeasurementTarget::SliderValue {
            slider_id: target,
            ..
        }) if target == slider_id
    );
    if !target_matches {
        measurement.begin_slider_value(slider_id, label, units, value);
    } else if slider.is_some_and(|slider| slider.is_changed()) {
        measurement.update_slider_value(slider_id, value);
    }
}

fn contact_nodes_from_selection(
    selection: &SelectionState,
) -> Result<(usize, Vec<fem_core::NodeId>), String> {
    let mut groups = selected_nodes_by_mesh(selection).into_iter();
    let Some((mesh_index, nodes)) = groups.next() else {
        return Err("Slave requires selected nodes".to_string());
    };
    if groups.next().is_some() {
        return Err("Select slave nodes from one mesh only".to_string());
    }

    Ok((mesh_index, nodes))
}

fn contact_surface_from_selection(
    model: &FemModel,
    selection: &SelectionState,
) -> Result<ContactDraftSurface, String> {
    let mut by_mesh = BTreeMap::<usize, Vec<FemEntityId>>::new();
    for target in &selection.targets {
        if matches!(
            target.entity,
            FemEntityId::Face(_) | FemEntityId::Element(_)
        ) {
            by_mesh
                .entry(target.mesh_index)
                .or_default()
                .push(target.entity);
        }
    }

    let mut surfaces = by_mesh.into_iter().filter_map(|(mesh_index, targets)| {
        let surfaces = model
            .meshes
            .get(mesh_index)?
            .surface_refs_from_targets(&targets);
        (!surfaces.is_empty()).then_some(ContactDraftSurface {
            mesh_index,
            surfaces,
        })
    });
    let Some(surface) = surfaces.next() else {
        return Err("Select boundary faces or surface elements first".to_string());
    };
    if surfaces.next().is_some() {
        return Err("Capture one mesh surface at a time".to_string());
    }

    Ok(surface)
}

pub(crate) fn capture_contact_side_button_system(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    mut settings: ResMut<ContactDefinitionSettings>,
    mut draft: ResMut<ContactDraftPreview>,
    mut filter: ResMut<SelectionFilter>,
    mut selection: ResMut<SelectionState>,
    mut candidates: ResMut<ContactCandidateState>,
    selected_query: Query<Entity, With<Selected>>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &CaptureContactSideButton,
        ),
        With<CaptureContactSideButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let result = match (button.0, settings.pair_kind, model.as_deref()) {
                (ContactCaptureSide::Slave, ContactPairKind::NodeSurface, _) => {
                    contact_nodes_from_selection(&selection).map(|(mesh_index, nodes)| {
                        draft.slave = Some(ContactDraftSlave::Nodes { mesh_index, nodes });
                        filter.level = SelectionLevel::Face;
                        "Slave nodes captured; select the master surface".to_string()
                    })
                }
                (ContactCaptureSide::Slave, ContactPairKind::SurfaceSurface, Some(model)) => {
                    contact_surface_from_selection(model, &selection).map(|surface| {
                        draft.slave = Some(ContactDraftSlave::Surface(surface));
                        filter.level = SelectionLevel::Face;
                        "Slave surface captured; select the master surface".to_string()
                    })
                }
                (ContactCaptureSide::Master, _, Some(model)) => {
                    contact_surface_from_selection(model, &selection).map(|surface| {
                        draft.master = Some(surface);
                        "Master surface captured; create the contact pair".to_string()
                    })
                }
                _ => Err("No model is loaded".to_string()),
            };

            match result {
                Ok(message) => {
                    settings.message = message;
                    candidates.candidates.clear();
                    candidates.selected = None;
                    selection.clear();
                    for entity in &selected_query {
                        commands.entity(entity).remove::<Selected>();
                    }
                }
                Err(message) => settings.message = message,
            }
        }

        let captured = match button.0 {
            ContactCaptureSide::Slave => draft.slave.is_some(),
            ContactCaptureSide::Master => draft.master.is_some(),
        };
        let captured_color = match button.0 {
            ContactCaptureSide::Slave => CONTACT_SLAVE_BUTTON,
            ContactCaptureSide::Master => CONTACT_MASTER_BUTTON,
        };
        *background = BackgroundColor(match (*interaction, captured) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => captured_color,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if captured {
            ACTIVE_BORDER
        } else {
            PANEL_BORDER
        });
    }
}

fn create_contact_from_draft(
    model: &mut FemModel,
    draft: &ContactDraftPreview,
    pair_kind: ContactPairKind,
    contact_type: ContactType,
    friction_coefficient: f32,
    penalty_factor: Option<f32>,
) -> Result<usize, String> {
    let master = draft
        .master
        .as_ref()
        .ok_or_else(|| "Capture the master surface first".to_string())?;
    let slave = draft
        .slave
        .as_ref()
        .ok_or_else(|| "Capture the slave side first".to_string())?;
    let slave_matches_kind = matches!(
        (pair_kind, slave),
        (
            ContactPairKind::NodeSurface,
            ContactDraftSlave::Nodes { .. }
        ) | (
            ContactPairKind::SurfaceSurface,
            ContactDraftSlave::Surface(_)
        )
    );
    if !slave_matches_kind {
        return Err("Captured slave does not match the selected topology".to_string());
    }
    let (friction_coefficient, penalty_factor) = match contact_type {
        ContactType::Tied => (0.0, None),
        ContactType::SmallSliding | ContactType::FiniteSliding => {
            if !friction_coefficient.is_finite() || friction_coefficient < 0.0 {
                return Err("Friction coefficient must be zero or greater".to_string());
            }
            if penalty_factor.is_some_and(|factor| !factor.is_finite() || factor <= 0.0) {
                return Err("Custom penalty factor must be greater than zero".to_string());
            }
            (friction_coefficient, penalty_factor)
        }
    };
    let pair_number = model.contacts.len() + 1;
    let pair_name = format!("CONTACT_{pair_number}");

    let master_ref = {
        let mesh = model
            .meshes
            .get_mut(master.mesh_index)
            .ok_or_else(|| "Master mesh no longer exists".to_string())?;
        let index = mesh.surface_sets.len();
        mesh.surface_sets.push(FemSurfaceSet {
            name: format!("{pair_name}_MASTER"),
            surfaces: master.surfaces.clone(),
        });
        fem_core::SurfaceSetRef::new(master.mesh_index, index)
    };

    let contact = match (pair_kind, slave) {
        (ContactPairKind::NodeSurface, ContactDraftSlave::Nodes { mesh_index, nodes }) => {
            let mesh = model
                .meshes
                .get_mut(*mesh_index)
                .ok_or_else(|| "Slave mesh no longer exists".to_string())?;
            let index = mesh.node_sets.len();
            mesh.node_sets.push(FemNodeSet {
                name: format!("{pair_name}_SLAVE"),
                nodes: nodes.clone(),
            });
            ContactPair::new_node_surface(
                pair_name.clone(),
                master_ref,
                fem_core::NodeSetRef::new(*mesh_index, index),
                contact_type,
            )
        }
        (ContactPairKind::SurfaceSurface, ContactDraftSlave::Surface(surface)) => {
            let mesh = model
                .meshes
                .get_mut(surface.mesh_index)
                .ok_or_else(|| "Slave mesh no longer exists".to_string())?;
            let index = mesh.surface_sets.len();
            mesh.surface_sets.push(FemSurfaceSet {
                name: format!("{pair_name}_SLAVE"),
                surfaces: surface.surfaces.clone(),
            });
            ContactPair::new(
                pair_name.clone(),
                master_ref,
                fem_core::SurfaceSetRef::new(surface.mesh_index, index),
                contact_type,
            )
        }
        _ => return Err("Captured slave does not match the selected topology".to_string()),
    };

    model
        .contacts
        .push(contact.with_contact_parameters(friction_coefficient, penalty_factor));
    Ok(model.contacts.len() - 1)
}

pub(crate) fn finalize_contact_button_system(
    mut model: Option<ResMut<FemModel>>,
    mut settings: ResMut<ContactDefinitionSettings>,
    mut draft: ResMut<ContactDraftPreview>,
    mut defined: ResMut<DefinedContactPreview>,
    mut candidates: ResMut<ContactCandidateState>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<FinalizeContactButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let friction_coefficient = if settings.contact_type == ContactType::Tied {
                0.0
            } else {
                slider_value(&sliders, SliderId::ContactFriction, 0.0)
            };
            let penalty_factor = (settings.contact_type != ContactType::Tied
                && settings.use_penalty_factor)
                .then(|| slider_value(&sliders, SliderId::ContactPenaltyFactor, 1.0e5));
            let result = model
                .as_deref_mut()
                .ok_or_else(|| "No model is loaded".to_string())
                .and_then(|model| {
                    create_contact_from_draft(
                        model,
                        &draft,
                        settings.pair_kind,
                        settings.contact_type,
                        friction_coefficient,
                        penalty_factor,
                    )
                });
            match result {
                Ok(index) => {
                    let name = model
                        .as_deref()
                        .and_then(|model| model.contacts.get(index))
                        .map(|contact| contact.name.clone())
                        .unwrap_or_else(|| "contact".to_string());
                    draft.clear();
                    defined.selected = Some(index);
                    candidates.candidates.clear();
                    candidates.selected = None;
                    settings.message = format!("Created {name}");
                }
                Err(message) => settings.message = message,
            }
        }

        let ready = draft.master.is_some() && draft.slave.is_some();
        *background = BackgroundColor(match (*interaction, ready) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if ready { ACTIVE_BORDER } else { PANEL_BORDER });
    }
}

pub(crate) fn update_contact_draft_status(
    settings: Res<ContactDefinitionSettings>,
    draft: Res<ContactDraftPreview>,
    mut query: Query<&mut Text, With<ContactDraftStatusText>>,
) {
    if !settings.is_changed() && !draft.is_changed() {
        return;
    }
    let Ok(mut text) = query.single_mut() else {
        return;
    };

    let slave = match draft.slave.as_ref() {
        Some(ContactDraftSlave::Nodes { mesh_index, nodes }) => {
            format!("Slave: {} nodes (part {})", nodes.len(), mesh_index + 1)
        }
        Some(ContactDraftSlave::Surface(surface)) => format!(
            "Slave: {} faces (part {})",
            surface.surfaces.len(),
            surface.mesh_index + 1
        ),
        None => "Slave: not set".to_string(),
    };
    let master = draft.master.as_ref().map_or_else(
        || "Master: not set".to_string(),
        |surface| {
            format!(
                "Master: {} faces (part {})",
                surface.surfaces.len(),
                surface.mesh_index + 1
            )
        },
    );
    **text = format!("{slave}\n{master}\n{}", settings.message);
}

/// Runs [`FemModel::find_contact_candidates`] against the current model and
/// stores the results (and selects the first one) in
/// [`ContactCandidateState`].
///
/// This is the "近接surfaceを自動検出 → 接触候補を提案" half of the
/// topology-aware contact workflow; [`accept_contact_button_system`]
/// implements the "ユーザーが数クリックで承認" half.
pub(crate) fn detect_contacts_button_system(
    model: Option<Res<FemModel>>,
    mut state: ResMut<ContactCandidateState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<DetectContactsButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(model) = model.as_deref() {
                state.refresh(model);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Materializes the currently selected [`ContactCandidate`] into a
/// [`ContactPair`](fem_core::ContactPair) via
/// [`FemModel::accept_contact_candidate`], then advances
/// [`ContactCandidateState`] to the next candidate.
pub(crate) fn accept_contact_button_system(
    mut model: Option<ResMut<FemModel>>,
    mut state: ResMut<ContactCandidateState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<AcceptContactButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let candidate = state.selected_candidate().cloned();

            if let (Some(model), Some(candidate)) = (model.as_deref_mut(), candidate) {
                let name = format!("CONTACT_{}", model.contacts.len() + 1);

                if model
                    .accept_contact_candidate(&candidate, name, ContactType::Tied)
                    .is_some()
                {
                    state.remove_selected();
                }
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Moves through detected contact candidates or rejects the current one
/// without changing the analysis model.
pub(crate) fn contact_candidate_action_button_system(
    mut state: ResMut<ContactCandidateState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &ContactCandidateActionButton,
        ),
        With<ContactCandidateActionButton>,
    >,
) {
    for (interaction, mut background, mut border, action) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            match action.0 {
                ContactCandidateAction::Previous => state.select_previous(),
                ContactCandidateAction::Next => state.select_next(),
                ContactCandidateAction::Reject => state.remove_selected(),
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Toggles whether parts outside the reviewed pair are rendered as a quiet,
/// transparent context shell.
pub(crate) fn contact_ghost_toggle_button_system(
    mut review: ResMut<ContactReviewSettings>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ContactGhostToggleButton>,
    >,
    mut labels: Query<&mut Text, With<ContactGhostToggleLabel>>,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            review.ghost_others = !review.ghost_others;
        }

        let active = review.ghost_others;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::None, false) => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });
    }

    for mut label in &mut labels {
        **label = format!(
            "Ghost others: {}",
            if review.ghost_others { "ON" } else { "OFF" }
        );
    }
}

/// Keeps visualization review state synchronized with the Contact page and
/// its view-only separation slider.
pub(crate) fn update_contact_review_settings(
    page: Res<SidebarPage>,
    candidates: Res<ContactCandidateState>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut review: ResMut<ContactReviewSettings>,
) {
    let active = *page == SidebarPage::Contact && candidates.selected_candidate().is_some();
    let separation_percent = sliders
        .iter()
        .find(|slider| slider.id == SliderId::ContactReviewSeparation)
        .map(|slider| slider.value)
        .unwrap_or(8.0);

    if review.active != active {
        review.active = active;
    }
    if (review.separation_percent - separation_percent).abs() > f32::EPSILON {
        review.separation_percent = separation_percent;
    }
}

/// Rebuilds the dynamic list of set buttons inside [`SetsListContainer`]
/// whenever [`FemModelVersion`] changes (new mesh loaded / imported /
/// reloaded).
///
/// Every node/element/surface set in every mesh of the model gets one
/// button, labelled with its name, kind, and member count.
pub(crate) fn rebuild_sets_list(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    version: Res<FemModelVersion>,
    mut last_version: Local<Option<u64>>,
    container_query: Query<Entity, With<SetsListContainer>>,
    children_query: Query<&Children>,
) {
    let current = version.value;
    let model_changed = model.as_ref().is_some_and(|m| m.is_changed());

    if *last_version == Some(current) && !model_changed {
        return;
    }

    *last_version = Some(current);

    let Ok(container) = container_query.single() else {
        return;
    };

    // Despawn the previous set of buttons.
    if let Ok(children) = children_query.get(container) {
        for &child in children {
            commands.entity(child).despawn();
        }
    }

    let Some(model) = model else {
        return;
    };

    commands.entity(container).with_children(|list| {
        for (mesh_index, mesh) in model.meshes.iter().enumerate() {
            for (set_index, set) in mesh.node_sets.iter().enumerate() {
                set_list_button(
                    list,
                    SetButton {
                        mesh_index,
                        kind: SetKind::Node,
                        set_index,
                    },
                    &format!("[N] {}  ({} nodes)", set.name, set.nodes.len()),
                );
            }

            for (set_index, set) in mesh.element_sets.iter().enumerate() {
                set_list_button(
                    list,
                    SetButton {
                        mesh_index,
                        kind: SetKind::Element,
                        set_index,
                    },
                    &format!("[E] {}  ({} elems)", set.name, set.elements.len()),
                );
            }

            for (set_index, set) in mesh.surface_sets.iter().enumerate() {
                set_list_button(
                    list,
                    SetButton {
                        mesh_index,
                        kind: SetKind::Surface,
                        set_index,
                    },
                    &format!("[S] {}  ({} faces)", set.name, set.surfaces.len()),
                );
            }
        }
    });
}

/// Rebuilds the read-only list of material/section entries inside
/// [`MaterialsSectionsListContainer`] whenever [`fem_core::AnalysisSetup`]
/// changes (typically after loading a `.cnt` file).
///
/// Unlike [`rebuild_sets_list`], these entries aren't clickable — there's
/// no mesh selection to drive from a material or section the way there is
/// from a node/element/surface set — so this just formats each one as a
/// text line.
/// Rebuilds the BC/load list whenever [`fem_core::AnalysisSetup`] changes.
pub(crate) fn rebuild_boundary_loads_list(
    mut commands: Commands,
    setup: Res<fem_core::AnalysisSetup>,
    container_query: Query<Entity, With<BoundaryLoadsListContainer>>,
    children_query: Query<&Children>,
) {
    if !setup.is_changed() {
        return;
    }

    let Ok(container) = container_query.single() else {
        return;
    };

    if let Ok(children) = children_query.get(container) {
        for &child in children {
            commands.entity(child).despawn();
        }
    }

    commands.entity(container).with_children(|list| {
        for (index, bc) in setup.boundary_conditions.iter().enumerate() {
            let label = format!(
                "[BC] {}  {}  ({} nodes)  val={:.4}",
                bc.name,
                bc.dof_label(),
                bc.nodes.len(),
                bc.value
            );
            setup_entry_row(
                list,
                &label,
                DeleteSetupEntry::BoundaryCondition(index),
                &format!("BC_{}", bc.name),
            );
        }

        // Group nodal loads by name for display (one entry per unique name).
        let mut seen_load_names: Vec<&str> = Vec::new();
        for (index, load) in setup.nodal_loads.iter().enumerate() {
            if seen_load_names.contains(&load.name.as_str()) {
                continue;
            }
            seen_load_names.push(&load.name);
            let dof_label = match load.dof {
                1 => "Fx",
                2 => "Fy",
                3 => "Fz",
                _ => "?",
            };
            let count = setup
                .nodal_loads
                .iter()
                .filter(|l| l.name == load.name)
                .count();
            let label = format!(
                "[Load] {}  {}={:.3}  ({} nodes)",
                load.name, dof_label, load.value, count
            );
            setup_entry_row(
                list,
                &label,
                DeleteSetupEntry::LoadGroup(index),
                &format!("Load_{}", load.name),
            );
        }

        for (index, dload) in setup.distributed_loads.iter().enumerate() {
            let kind_label = match dload.kind {
                fem_core::DistributedLoadKind::Pressure => "Pressure",
                fem_core::DistributedLoadKind::Gravity => "Gravity",
            };
            let unit = match dload.kind {
                fem_core::DistributedLoadKind::Pressure => "faces",
                fem_core::DistributedLoadKind::Gravity => "elems",
            };
            let label = format!(
                "[DLoad] {}  {kind_label}={:.3}  ({} {unit})",
                dload.name,
                dload.value,
                dload.target.len()
            );
            setup_entry_row(
                list,
                &label,
                DeleteSetupEntry::DistributedLoad(index),
                &format!("DLoad_{}", dload.name),
            );
        }

        if setup.boundary_conditions.is_empty()
            && setup.nodal_loads.is_empty()
            && setup.distributed_loads.is_empty()
        {
            list.spawn((
                Text::new("(none yet - select nodes and use buttons above)"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
        }
    });
}

/// Applies a constraint preset to all currently selected nodes.
///
/// One [`BoundaryCondition`] entry is created per contiguous call, named
/// `BC1`, `BC2`, … by [`AnalysisSetup::next_auto_name`]. Using a separate
/// entry per click (rather than merging into an existing one) keeps the
/// list simple and undo straightforward: delete the most-recent entry to
/// revert the action.
// ── surface selection growth ─────────────────────────────────────────────────

pub(crate) fn surface_selection_mode_button_system(
    filter: Res<SelectionFilter>,
    mut settings: ResMut<SurfaceSelectionSettings>,
    mut buttons: Query<(
        Ref<Interaction>,
        &SurfaceSelectionModeButton,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut angle_controls: Query<&mut Node, With<SurfaceAngleControls>>,
    mut surface_controls: Query<
        &mut Node,
        (
            With<SurfaceSelectionControls>,
            Without<SurfaceAngleControls>,
            Without<SurfaceSelectionUnavailableHint>,
        ),
    >,
    mut unavailable_hints: Query<
        (&mut Node, &mut Text),
        (
            With<SurfaceSelectionUnavailableHint>,
            Without<SurfaceSelectionControls>,
            Without<SurfaceAngleControls>,
        ),
    >,
) {
    let supports_growth = supports_surface_growth(filter.level);

    for (interaction, button, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            settings.mode = button.mode;
        }

        let active = settings.mode == button.mode;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *background = BackgroundColor(color);
        *border = if active {
            BorderColor::all(ACTIVE_BORDER)
        } else {
            BorderColor::all(PANEL_BORDER)
        };
    }

    for mut node in &mut surface_controls {
        node.display = if supports_growth {
            Display::Flex
        } else {
            Display::None
        };
    }

    for (mut node, mut text) in &mut unavailable_hints {
        node.display = if supports_growth {
            Display::None
        } else {
            Display::Flex
        };
        **text = match filter.level {
            SelectionLevel::Node => {
                "Node: single item; double/triple click expands connectivity".to_string()
            }
            SelectionLevel::Edge => {
                "Edge: click follows a continuous feature line; double click includes branches"
                    .to_string()
            }
            SelectionLevel::Face | SelectionLevel::Element => String::new(),
        };
    }

    for mut node in &mut angle_controls {
        node.display = if supports_growth && settings.mode == SurfaceSelectionMode::Smooth {
            Display::Flex
        } else {
            Display::None
        };
    }
}

const fn supports_surface_growth(level: SelectionLevel) -> bool {
    matches!(level, SelectionLevel::Face | SelectionLevel::Element)
}

/// Computes the live "what would clicking select right now" preview group
/// (see [`fem_core::HoverPreviewTargets`]) from the current hover target
/// and the active Single/Coplanar/Smooth mode, every frame.
///
/// This always walks from whatever is under the cursor *this* frame,
/// independent of what's already selected — [`selection::click_selection_system`]
/// is what actually commits this preview into [`SelectionState`] on click
/// (respecting the shared replace/add/toggle/remove modifier rules),
/// so there's no separate click-time seed to track here.
pub(crate) fn update_hover_preview_group(
    hover: Res<HoverResult>,
    settings: Res<SurfaceSelectionSettings>,
    model: Option<Res<FemModel>>,
    slider_query: Query<&SliderState, With<SliderTrack>>,
    mut preview: ResMut<fem_core::HoverPreviewTargets>,
) {
    let (new_targets, new_highlight_targets): (Vec<_>, Vec<_>) = match hover.hit {
        None => (Vec::new(), Vec::new()),

        Some(hit) if matches!(hit.target.entity, FemEntityId::Edge(_)) => {
            let target = hit.target;
            let FemEntityId::Edge(edge_id) = target.entity else {
                unreachable!();
            };
            let edge_targets: Vec<_> = model
                .as_deref()
                .and_then(|model| model.meshes.get(target.mesh_index))
                .map(|mesh| {
                    fem_core::expand_continuous_feature_edges(
                        mesh,
                        edge_id,
                        fem_core::DEFAULT_FEATURE_EDGE_ANGLE_DEG,
                    )
                    .into_iter()
                    .map(|id| FemEntityRef::edge(target.mesh_index, id))
                    .collect()
                })
                .unwrap_or_else(|| vec![target]);
            (edge_targets.clone(), edge_targets)
        }

        Some(hit) if settings.mode == SurfaceSelectionMode::Single => {
            (vec![hit.target], vec![hit.target])
        }

        Some(hit) => {
            let target = hit.target;
            let threshold = match settings.mode {
                SurfaceSelectionMode::Coplanar => COPLANAR_TOLERANCE_DEG,
                SurfaceSelectionMode::Smooth => slider_query
                    .iter()
                    .find(|slider| slider.id == SliderId::SurfaceAngle)
                    .map(|slider| slider.value)
                    .unwrap_or(DEFAULT_SMOOTH_ANGLE_DEG),
                SurfaceSelectionMode::Single => unreachable!(),
            };

            let Some(mesh) = model
                .as_deref()
                .and_then(|model| model.meshes.get(target.mesh_index))
            else {
                let fallback = vec![target];
                if preview.targets != fallback || preview.highlight_targets != fallback {
                    preview.targets = fallback.clone();
                    preview.highlight_targets = fallback;
                }
                return;
            };

            match target.entity {
                fem_core::FemEntityId::Face(fid) => {
                    let (faces, _) = match settings.mode {
                        SurfaceSelectionMode::Coplanar => {
                            fem_core::expand_coplanar_from_face(mesh, fid, threshold)
                        }
                        SurfaceSelectionMode::Smooth => {
                            fem_core::expand_smooth_from_face(mesh, fid, threshold)
                        }
                        SurfaceSelectionMode::Single => unreachable!(),
                    };
                    let face_targets: Vec<_> = faces
                        .into_iter()
                        .map(|id| fem_core::FemEntityRef::face(target.mesh_index, id))
                        .collect();
                    (face_targets.clone(), face_targets)
                }
                fem_core::FemEntityId::Element(eid) => {
                    // Always commit `Element` targets here, matching the
                    // seed's own kind — never `Face`. Surface-growth functions
                    // return `faces` only as an internal detail of how they
                    // compute the group (they walk by face, since that's
                    // where normals live). Those faces now become the overlay
                    // geometry while the committed targets remain Elements,
                    // avoiding a misleading display of every internal
                    // tetrahedron side. Keeping the committed target kind pure
                    // is required by element-group export and setup operations.
                    let seed_face = hit.surface_face.filter(|face_id| {
                        mesh.cached_boundary_faces()
                            .iter()
                            .any(|face| face.id == *face_id && face.element == Some(eid))
                    });
                    let (faces, elements) = match (settings.mode, seed_face) {
                        (SurfaceSelectionMode::Coplanar, Some(face_id)) => {
                            fem_core::expand_coplanar_from_face(mesh, face_id, threshold)
                        }
                        (SurfaceSelectionMode::Coplanar, None) => {
                            fem_core::expand_coplanar_from_element(mesh, eid, threshold)
                        }
                        (SurfaceSelectionMode::Smooth, Some(face_id)) => {
                            fem_core::expand_smooth_from_face(mesh, face_id, threshold)
                        }
                        (SurfaceSelectionMode::Smooth, None) => {
                            fem_core::expand_smooth_from_element(mesh, eid, threshold)
                        }
                        (SurfaceSelectionMode::Single, _) => unreachable!(),
                    };
                    let element_targets = elements
                        .into_iter()
                        .map(|id| fem_core::FemEntityRef::element(target.mesh_index, id))
                        .collect();
                    let face_targets = faces
                        .into_iter()
                        .map(|id| fem_core::FemEntityRef::face(target.mesh_index, id))
                        .collect();
                    (element_targets, face_targets)
                }
                _ => (vec![target], vec![target]),
            }
        }
    };

    // Avoid marking the resource `Changed` (and so triggering a highlight
    // mesh rebuild) every single frame when nothing actually moved.
    if preview.targets != new_targets || preview.highlight_targets != new_highlight_targets {
        preview.targets = new_targets;
        preview.highlight_targets = new_highlight_targets;
    }
}

/// Explains both the growth algorithm and the Face/Element target distinction.
pub(crate) fn update_surface_selection_hint(
    filter: Res<SelectionFilter>,
    settings: Res<SurfaceSelectionSettings>,
    mut query: Query<&mut Text, With<SurfaceSelectionHint>>,
) {
    if !filter.is_changed() && !settings.is_changed() {
        return;
    }

    let Ok(mut text) = query.single_mut() else {
        return;
    };

    **text = surface_selection_hint(filter.level, settings.mode).to_string();
}

fn surface_selection_hint(level: SelectionLevel, mode: SurfaceSelectionMode) -> &'static str {
    match (level, mode) {
        (SelectionLevel::Face, SurfaceSelectionMode::Single) => {
            "Face Single = one boundary surface face"
        }
        (SelectionLevel::Element, SurfaceSelectionMode::Single) => {
            "Element Single = one whole FEM element"
        }
        (SelectionLevel::Face, SurfaceSelectionMode::Coplanar) => {
            "Face Coplanar = flat patch (fixed 0.5° tolerance)"
        }
        (SelectionLevel::Element, SurfaceSelectionMode::Coplanar) => {
            "Element Coplanar = volumes; bodies extend behind surface"
        }
        (SelectionLevel::Face, SurfaceSelectionMode::Smooth) => {
            "Face Smooth = connected curved surface patch"
        }
        (SelectionLevel::Element, SurfaceSelectionMode::Smooth) => {
            "Element Smooth = whole elements behind curved patch"
        }
        (_, SurfaceSelectionMode::Single) => "Single selects one topology item",
        (_, SurfaceSelectionMode::Coplanar) => "Coplanar applies to Face and Element",
        (_, SurfaceSelectionMode::Smooth) => "Smooth applies to Face and Element",
    }
}

/// Updates the `SelectionInfoText` every frame with:
///   "37 nodes selected  ·  Hover: Node 412 (x=12.3, y=0.0, z=-5.1)"
///
/// This is the single most important piece of contextual feedback in the
/// pre-process workflow: a person must know *what is selected* before
/// clicking a boundary-condition or load preset.
pub(crate) fn update_selection_info_text(
    selection: Res<SelectionState>,
    hover: Res<HoverResult>,
    model: Option<Res<FemModel>>,
    mut query: Query<&mut Text, With<SelectionInfoText>>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };

    // Count selected entities by type.
    let node_count = selection
        .targets
        .iter()
        .filter(|t| matches!(t.entity, fem_core::FemEntityId::Node(_)))
        .count();
    let elem_count = selection
        .targets
        .iter()
        .filter(|t| matches!(t.entity, fem_core::FemEntityId::Element(_)))
        .count();

    let sel_part = match (node_count, elem_count) {
        (0, 0) => "Nothing selected".to_string(),
        (n, 0) => format!("{n} node{} selected", if n == 1 { "" } else { "s" }),
        (0, e) => format!("{e} element{} selected", if e == 1 { "" } else { "s" }),
        (n, e) => format!(
            "{n} node{}, {e} elem{} selected",
            if n == 1 { "" } else { "s" },
            if e == 1 { "" } else { "s" }
        ),
    };

    // Hover info: show node XYZ when hovering a node.
    let hover_part = hover
        .hit
        .and_then(|hit| {
            let fem_core::FemEntityId::Node(node_id) = hit.target.entity else {
                return None;
            };
            let mesh = model.as_deref()?.meshes.get(hit.target.mesh_index)?;
            mesh.node_position(node_id).map(|pos| {
                format!(
                    "  |  Node {} ({:.3}, {:.3}, {:.3})",
                    node_id.0, pos.x, pos.y, pos.z
                )
            })
        })
        .unwrap_or_default();

    **text = format!("{sel_part}{hover_part}");
}

/// Updates the text inside each [`ConstraintPresetButton`] to show the
/// current selected-node count, e.g. "Fix XYZ (37)".
/// When 0 nodes are selected, the button is dimmed.
pub(crate) fn update_constraint_button_labels(
    selection: Res<SelectionState>,
    buttons: Query<(&ConstraintPresetButton, &Children), Without<ConstraintPresetLabel>>,
    mut labels: Query<&mut Text, With<ConstraintPresetLabel>>,
) {
    if !selection.is_changed() {
        return;
    }

    let n = selection
        .targets
        .iter()
        .filter(|t| matches!(t.entity, fem_core::FemEntityId::Node(_)))
        .count();

    for (btn, children) in &buttons {
        for &child in children {
            if let Ok(mut text) = labels.get_mut(child) {
                **text = if n > 0 {
                    format!("{} ({})", btn.label, n)
                } else {
                    btn.label.to_string()
                };
            }
        }
    }
}

/// Updates the "Apply Load" button label with the node count.
pub(crate) fn update_apply_load_label(
    selection: Res<SelectionState>,
    selected_dir: Res<SelectedLoadDirection>,
    slider_query: Query<&SliderState, With<SliderTrack>>,
    mut labels: Query<&mut Text, With<ApplyLoadLabel>>,
) {
    if !selection.is_changed() && !selected_dir.is_changed() {
        return;
    }

    let Ok(mut text) = labels.single_mut() else {
        return;
    };

    let n = selection
        .targets
        .iter()
        .filter(|t| matches!(t.entity, fem_core::FemEntityId::Node(_)))
        .count();

    let mag = slider_query
        .iter()
        .find(|s| s.id == SliderId::LoadMagnitude)
        .map(|s| s.value)
        .unwrap_or(100.0);

    let dir_label = selected_dir
        .0
        .map(|(dof, sign)| {
            let axis = ["?", "X", "Y", "Z"]
                .get(dof as usize)
                .copied()
                .unwrap_or("?");
            let sign_char = if sign >= 0.0 { "+" } else { "-" };
            format!(" {sign_char}{axis} {mag:.0}")
        })
        .unwrap_or_else(|| " (pick direction)".to_string());

    **text = if n > 0 {
        format!("Apply Load{dir_label}  ({n} nodes)")
    } else {
        format!("Apply Load{dir_label}  - no nodes selected")
    };
}

/// Clears all boundary conditions and nodal loads at once.
pub(crate) fn clear_all_bc_loads_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut buttons: Query<(Ref<Interaction>, &mut BackgroundColor), With<ClearAllBcLoadsButton>>,
) {
    for (interaction, mut bg) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            setup.boundary_conditions.clear();
            setup.nodal_loads.clear();
            setup.distributed_loads.clear();
            setup.set_changed();
        }

        *bg = BackgroundColor(match *interaction {
            Interaction::Pressed | Interaction::Hovered => Color::srgba(0.60, 0.15, 0.15, 0.95),
            Interaction::None => Color::srgba(0.40, 0.12, 0.12, 0.80),
        });
    }
}

fn selected_nodes_by_mesh(selection: &SelectionState) -> BTreeMap<usize, Vec<fem_core::NodeId>> {
    let mut by_mesh = BTreeMap::<usize, BTreeSet<fem_core::NodeId>>::new();

    for target in &selection.targets {
        if let FemEntityId::Node(id) = target.entity {
            by_mesh.entry(target.mesh_index).or_default().insert(id);
        }
    }

    by_mesh
        .into_iter()
        .map(|(mesh_index, nodes)| (mesh_index, nodes.into_iter().collect()))
        .collect()
}

pub(crate) fn constraint_preset_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    model: Option<Res<FemModel>>,
    selection: Res<SelectionState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &ConstraintPresetButton,
        ),
        With<ConstraintPresetButton>,
    >,
) {
    let Some(model) = model else {
        return;
    };

    for (interaction, mut bg, mut border, preset) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            for (mesh_index, nodes) in selected_nodes_by_mesh(&selection) {
                if nodes.is_empty() || model.meshes.get(mesh_index).is_none() {
                    continue;
                }

                let bc_name = setup.next_auto_name_pub("BC");

                setup.boundary_conditions.push(fem_core::BoundaryCondition {
                    name: bc_name,
                    mesh_index,
                    nodes,
                    ngrp_name: None,
                    dof_start: preset.dof_start,
                    dof_end: preset.dof_end,
                    value: 0.0,
                });
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Toggles the load direction selection; tracks active direction in
/// [`SelectedLoadDirection`] and highlights the active button.
pub(crate) fn load_direction_button_system(
    mut selected: ResMut<SelectedLoadDirection>,
    mut active_editor: ResMut<ActiveLoadEditor>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &LoadDirectionButton,
        ),
        With<LoadDirectionButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let new_dir = (btn.dof, btn.sign);
            if selected.0 == Some(new_dir) {
                selected.0 = None; // toggle off
                *active_editor = ActiveLoadEditor::None;
                measurement.clear();
            } else {
                selected.0 = Some(new_dir);
                *active_editor = ActiveLoadEditor::Nodal;
                measurement.begin_slider_value(
                    SliderId::LoadMagnitude,
                    nodal_load_measurement_label(btn.dof, btn.sign),
                    "analysis force units",
                    slider_value(&sliders, SliderId::LoadMagnitude, 100.0),
                );
            }
        }

        let active = selected.0 == Some((btn.dof, btn.sign));
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };

        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

fn nodal_load_measurement_label(dof: u8, sign: f32) -> &'static str {
    match (dof, sign >= 0.0) {
        (1, true) => "Nodal load +X",
        (1, false) => "Nodal load -X",
        (2, true) => "Nodal load +Y",
        (2, false) => "Nodal load -Y",
        (3, true) => "Nodal load +Z",
        (3, false) => "Nodal load -Z",
        _ => "Nodal load",
    }
}

fn slider_value(
    sliders: &Query<&SliderState, With<SliderTrack>>,
    id: SliderId,
    fallback: f32,
) -> f32 {
    sliders
        .iter()
        .find(|slider| slider.id == id)
        .map(|slider| slider.value)
        .unwrap_or(fallback)
}

/// Applies the selected direction + slider magnitude as a nodal load to
/// every currently selected node.
pub(crate) fn apply_load_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    model: Option<Res<FemModel>>,
    selection: Res<SelectionState>,
    selected_dir: Res<SelectedLoadDirection>,
    slider_query: Query<&SliderState, With<SliderTrack>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ApplyLoadButton>,
    >,
) {
    let Some(model) = model else {
        return;
    };

    let magnitude = slider_query
        .iter()
        .find(|s| s.id == SliderId::LoadMagnitude)
        .map(|s| s.value)
        .unwrap_or(100.0);

    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let Some((dof, sign)) = selected_dir.0 else {
                continue;
            };

            let value = magnitude * sign;

            for (mesh_index, nodes) in selected_nodes_by_mesh(&selection) {
                if nodes.is_empty() || model.meshes.get(mesh_index).is_none() {
                    continue;
                }

                let name = setup.next_auto_name_pub("LOAD");

                for node in nodes {
                    setup.nodal_loads.push(fem_core::NodalLoad {
                        name: name.clone(),
                        mesh_index,
                        node,
                        ngrp_name: None,
                        dof,
                        value,
                    });
                }
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Adds one of the built-in material presets to [`AnalysisSetup`]. If a
/// material with the same name already exists the button is a no-op (to
/// avoid duplicate entries cluttering the list).
/// Toggles the active [`SelectedDloadKind`] when [Pressure]/[Gravity] clicked.
pub(crate) fn dload_kind_button_system(
    mut selected: ResMut<SelectedDloadKind>,
    mut active_editor: ResMut<ActiveLoadEditor>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &DloadKindButton,
        ),
        With<DloadKindButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            *selected = btn.0;
            *active_editor = ActiveLoadEditor::Distributed;
            let (label, units) = match btn.0 {
                SelectedDloadKind::Pressure => ("Pressure", "analysis pressure units"),
                SelectedDloadKind::Gravity => ("Gravity acceleration", "analysis accel. units"),
            };
            measurement.begin_slider_value(
                SliderId::DloadMagnitude,
                label,
                units,
                slider_value(&sliders, SliderId::DloadMagnitude, 1.0),
            );
        }

        let active = *selected == btn.0;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Keeps the shared lower-right numeric field attached to whichever load
/// authoring control was used most recently.
pub(crate) fn sync_load_measurement_box(
    page: Res<SidebarPage>,
    active_editor: Res<ActiveLoadEditor>,
    selected_direction: Res<SelectedLoadDirection>,
    kind: Res<SelectedDloadKind>,
    sliders: Query<Ref<SliderState>, With<SliderTrack>>,
    mut measurement: ResMut<MeasurementBoxState>,
) {
    if *page != SidebarPage::Loads {
        return;
    }

    let (slider_id, label, units, fallback) = match *active_editor {
        ActiveLoadEditor::None => return,
        ActiveLoadEditor::Nodal => {
            let Some((dof, sign)) = selected_direction.0 else {
                return;
            };
            (
                SliderId::LoadMagnitude,
                nodal_load_measurement_label(dof, sign),
                "analysis force units",
                100.0,
            )
        }
        ActiveLoadEditor::Distributed => match *kind {
            SelectedDloadKind::Pressure => (
                SliderId::DloadMagnitude,
                "Pressure",
                "analysis pressure units",
                1.0,
            ),
            SelectedDloadKind::Gravity => (
                SliderId::DloadMagnitude,
                "Gravity acceleration",
                "analysis accel. units",
                1.0,
            ),
        },
    };

    let slider = sliders.iter().find(|slider| slider.id == slider_id);
    let value = slider
        .as_ref()
        .map(|slider| slider.value)
        .unwrap_or(fallback);
    let target_matches = matches!(
        measurement.target,
        Some(MeasurementTarget::SliderValue {
            slider_id: target,
            ..
        }) if target == slider_id
    );

    if !target_matches {
        measurement.begin_slider_value(slider_id, label, units, value);
    } else if slider.is_some_and(|slider| slider.is_changed()) {
        measurement.update_slider_value(slider_id, value);
    }
}

/// Builds provisional load arrows from the live selection. The resulting
/// resource is view-only; Apply remains the explicit commit boundary.
pub(crate) fn update_boundary_load_preview(
    page: Res<SidebarPage>,
    active_editor: Res<ActiveLoadEditor>,
    selected_direction: Res<SelectedLoadDirection>,
    kind: Res<SelectedDloadKind>,
    selection: Res<SelectionState>,
    model: Option<Res<FemModel>>,
    sliders: Query<Ref<SliderState>, With<SliderTrack>>,
    mut preview: ResMut<BoundaryLoadPreview>,
) {
    let slider_changed = sliders.iter().any(|slider| {
        matches!(
            slider.id,
            SliderId::LoadMagnitude | SliderId::DloadMagnitude
        ) && slider.is_changed()
    });
    let model_changed = model.as_ref().is_some_and(|model| model.is_changed());
    if !page.is_changed()
        && !active_editor.is_changed()
        && !selected_direction.is_changed()
        && !kind.is_changed()
        && !selection.is_changed()
        && !slider_changed
        && !model_changed
    {
        return;
    }

    let Some(model) = model.as_deref() else {
        if *preview != BoundaryLoadPreview::default() {
            *preview = BoundaryLoadPreview::default();
        }
        return;
    };
    if *page != SidebarPage::Loads {
        if *preview != BoundaryLoadPreview::default() {
            *preview = BoundaryLoadPreview::default();
        }
        return;
    }

    let mut next = BoundaryLoadPreview::default();

    match *active_editor {
        ActiveLoadEditor::None => {}
        ActiveLoadEditor::Nodal => {
            let Some((dof, sign)) = selected_direction.0 else {
                if *preview != next {
                    *preview = next;
                }
                return;
            };
            let magnitude = sliders
                .iter()
                .find(|slider| slider.id == SliderId::LoadMagnitude)
                .map(|slider| slider.value)
                .unwrap_or(100.0);
            let axis = match dof {
                1 => Vec3::X,
                2 => Vec3::Y,
                3 => Vec3::Z,
                _ => Vec3::ZERO,
            };
            let direction = signed_preview_direction(axis * sign, magnitude);
            next.kind = Some(BoundaryLoadPreviewKind::Nodal);
            next.arrows = selection
                .targets
                .iter()
                .filter_map(|target| {
                    let FemEntityId::Node(node_id) = target.entity else {
                        return None;
                    };
                    Some(BoundaryLoadPreviewArrow {
                        origin: model
                            .meshes
                            .get(target.mesh_index)?
                            .node_position(node_id)?,
                        direction,
                    })
                })
                .collect();
        }
        ActiveLoadEditor::Distributed => {
            let magnitude = sliders
                .iter()
                .find(|slider| slider.id == SliderId::DloadMagnitude)
                .map(|slider| slider.value)
                .unwrap_or(1.0);

            match *kind {
                SelectedDloadKind::Pressure => {
                    next.kind = Some(BoundaryLoadPreviewKind::Pressure);
                    for (mesh_index, face_refs) in
                        selected_faces_from_faces_or_elements(&selection, model)
                    {
                        let Some(mesh) = model.meshes.get(mesh_index) else {
                            continue;
                        };
                        let selected: BTreeSet<_> = face_refs.into_iter().collect();
                        next.arrows.extend(
                            mesh.cached_boundary_faces()
                                .iter()
                                .filter(|face| {
                                    face.element_face_ref()
                                        .is_some_and(|face_ref| selected.contains(&face_ref))
                                })
                                .filter_map(|face| mesh.face_geometry(face))
                                .map(|geometry| BoundaryLoadPreviewArrow {
                                    origin: geometry.centroid,
                                    direction: signed_preview_direction(
                                        -geometry.normal,
                                        magnitude,
                                    ),
                                }),
                        );
                    }
                }
                SelectedDloadKind::Gravity => {
                    next.kind = Some(BoundaryLoadPreviewKind::Gravity);
                    for (mesh_index, element_ids) in
                        selected_elements_from_faces_or_elements(&selection, model)
                    {
                        let Some(mesh) = model.meshes.get(mesh_index) else {
                            continue;
                        };
                        let selected: BTreeSet<_> = element_ids.into_iter().collect();
                        let mut centroid = Vec3::ZERO;
                        let mut count = 0usize;
                        for element in &mesh.elements {
                            if !selected.contains(&element.id) {
                                continue;
                            }
                            if let Some(positions) = mesh.node_positions(&element.nodes) {
                                for position in positions {
                                    centroid += position;
                                    count += 1;
                                }
                            }
                        }
                        if count > 0 {
                            next.arrows.push(BoundaryLoadPreviewArrow {
                                origin: centroid / count as f32,
                                direction: signed_preview_direction(Vec3::NEG_Y, magnitude),
                            });
                        }
                    }
                }
            }
        }
    }

    if *preview != next {
        *preview = next;
    }
}

fn signed_preview_direction(direction: Vec3, magnitude: f32) -> Vec3 {
    if magnitude.abs() <= f32::EPSILON {
        Vec3::ZERO
    } else if magnitude < 0.0 {
        -direction
    } else {
        direction
    }
}

/// Resolves the parent element of every currently selected face.
///
/// Faces selected via the Face filter carry an `element: Option<ElementId>`
/// back-reference set when boundary faces were cached; this just gathers
/// the unique non-`None` values. If the person has Element filter active and
/// selected elements directly instead, those are used as a fallback so the
/// Apply button still does something sensible regardless of filter mode.
fn selected_elements_from_faces_or_elements(
    selection: &SelectionState,
    model: &FemModel,
) -> BTreeMap<usize, Vec<fem_core::ElementId>> {
    let mut by_mesh = BTreeMap::<usize, BTreeSet<fem_core::ElementId>>::new();

    for target in &selection.targets {
        let Some(mesh) = model.meshes.get(target.mesh_index) else {
            continue;
        };

        let element = match target.entity {
            FemEntityId::Face(id) => mesh
                .cached_boundary_faces()
                .iter()
                .find(|face| face.id == id)
                .and_then(|face| face.element),
            FemEntityId::Element(id) => Some(id),
            FemEntityId::Node(_) | FemEntityId::Edge(_) => None,
        };

        if let Some(element) = element {
            by_mesh
                .entry(target.mesh_index)
                .or_default()
                .insert(element);
        }
    }

    by_mesh
        .into_iter()
        .map(|(mesh_index, elements)| (mesh_index, elements.into_iter().collect()))
        .collect()
}

/// Resolves the current selection to boundary element-faces (element +
/// local face index), for pressure [`fem_core::DistributedLoad`]s — unlike
/// [`selected_elements_from_faces_or_elements`], this keeps which specific
/// local face was picked, since FrontISTR's pressure `!DLOAD` needs that
/// (`P1`..`P6`) per element.
///
/// Delegates to [`fem_core::FemMesh::surface_refs_from_targets`] (the same
/// resolution `create_surface_button_system` uses for contact surface
/// sets): a `Face` target maps directly to its owning element's face, and
/// an `Element` target expands to every boundary face of that element.
///
fn selected_faces_from_faces_or_elements(
    selection: &SelectionState,
    model: &FemModel,
) -> BTreeMap<usize, Vec<fem_core::ElementFaceRef>> {
    let mut by_mesh = BTreeMap::<usize, BTreeSet<fem_core::ElementFaceRef>>::new();

    for target in &selection.targets {
        let Some(mesh) = model.meshes.get(target.mesh_index) else {
            continue;
        };

        for face_ref in mesh.surface_refs_from_targets(&[target.entity]) {
            by_mesh
                .entry(target.mesh_index)
                .or_default()
                .insert(face_ref);
        }
    }

    by_mesh
        .into_iter()
        .map(|(mesh_index, faces)| (mesh_index, faces.into_iter().collect()))
        .collect()
}

/// Updates the [`ApplyDloadButton`]'s label with the current face/element
/// count, mirroring [`update_apply_load_label`]'s feedback pattern.
pub(crate) fn update_apply_dload_label(
    selection: Res<SelectionState>,
    model: Option<Res<FemModel>>,
    kind: Res<SelectedDloadKind>,
    slider_query: Query<&SliderState, With<SliderTrack>>,
    mut labels: Query<&mut Text, With<ApplyDloadLabel>>,
) {
    if !selection.is_changed() && !kind.is_changed() {
        return;
    }

    let Ok(mut text) = labels.single_mut() else {
        return;
    };
    let Some(model) = model.as_deref() else {
        return;
    };

    // Pressure counts picked *faces* (what actually gets written to the
    // .cnt); gravity counts elements, since it has no face to speak of.
    let (n, unit) = match *kind {
        SelectedDloadKind::Pressure => (
            selected_faces_from_faces_or_elements(&selection, model)
                .values()
                .map(Vec::len)
                .sum::<usize>(),
            "faces",
        ),
        SelectedDloadKind::Gravity => (
            selected_elements_from_faces_or_elements(&selection, model)
                .values()
                .map(Vec::len)
                .sum::<usize>(),
            "elements",
        ),
    };

    let mag = slider_query
        .iter()
        .find(|s| s.id == SliderId::DloadMagnitude)
        .map(|s| s.value)
        .unwrap_or(1.0);

    let kind_label = match *kind {
        SelectedDloadKind::Pressure => "Pressure",
        SelectedDloadKind::Gravity => "Gravity",
    };

    **text = if n > 0 {
        format!("Apply {kind_label} {mag:.2}  ({n} {unit})")
    } else {
        format!("Apply {kind_label}  - no faces/elements selected")
    };
}

/// Creates a [`fem_core::DistributedLoad`] from the currently selected faces
/// (resolved to their parent elements) and the configured kind/magnitude.
pub(crate) fn apply_dload_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    model: Option<Res<FemModel>>,
    selection: Res<SelectionState>,
    kind: Res<SelectedDloadKind>,
    slider_query: Query<&SliderState, With<SliderTrack>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ApplyDloadButton>,
    >,
) {
    let Some(model) = model.as_deref() else {
        return;
    };

    let magnitude = slider_query
        .iter()
        .find(|s| s.id == SliderId::DloadMagnitude)
        .map(|s| s.value)
        .unwrap_or(1.0);

    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            // Pressure needs which face was picked (P1..P6 in the exported
            // .cnt); gravity is a whole-element body force and has no face.
            match *kind {
                SelectedDloadKind::Pressure => {
                    for (mesh_index, faces) in
                        selected_faces_from_faces_or_elements(&selection, model)
                    {
                        if faces.is_empty() {
                            continue;
                        }

                        let name = setup.next_auto_name_pub("DLOAD");
                        setup.distributed_loads.push(fem_core::DistributedLoad {
                            name,
                            mesh_index,
                            target: fem_core::DistributedLoadTarget::Faces(faces),
                            kind: fem_core::DistributedLoadKind::Pressure,
                            value: magnitude,
                            direction: None,
                        });
                    }
                }
                SelectedDloadKind::Gravity => {
                    for (mesh_index, elements) in
                        selected_elements_from_faces_or_elements(&selection, model)
                    {
                        if elements.is_empty() {
                            continue;
                        }

                        let name = setup.next_auto_name_pub("DLOAD");
                        setup.distributed_loads.push(fem_core::DistributedLoad {
                            name,
                            mesh_index,
                            target: fem_core::DistributedLoadTarget::Elements(elements),
                            kind: fem_core::DistributedLoadKind::Gravity,
                            value: magnitude,
                            direction: Some(Vec3::NEG_Y),
                        });
                    }
                }
            }

            setup.set_changed();
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn analysis_type_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &AnalysisTypeButton,
        ),
        With<AnalysisTypeButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            setup.solver.analysis_type = btn.0;
        }
        let active = setup.solver.analysis_type == btn.0;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn solver_method_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &SolverMethodButton,
        ),
        With<SolverMethodButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            setup.solver.solver_method = btn.0;
        }
        let active = setup.solver.solver_method == btn.0;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn material_preset_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &MaterialPresetButton,
        ),
        With<MaterialPresetButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(preset) = material_presets().get(btn.preset_index) {
                if setup.material_by_name(preset.name).is_none() {
                    setup.add_material(
                        preset.name,
                        Some(preset.young_modulus),
                        Some(preset.poisson_ratio),
                        Some(preset.density),
                    );
                }
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Handles clicks on the ✕ delete buttons in the BC/load and
/// material/section lists, removing the corresponding entry from
/// [`AnalysisSetup`]. Changes trigger `is_changed()` on the resource so
/// the list-rebuild systems re-run automatically and the 3D boundary
/// visuals respawn.
pub(crate) fn delete_setup_entry_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &DeleteSetupEntry),
        With<DeleteSetupEntry>,
    >,
) {
    for (interaction, mut bg, entry) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            match *entry {
                DeleteSetupEntry::BoundaryCondition(i) => setup.remove_boundary_condition(i),
                DeleteSetupEntry::LoadGroup(i) => setup.remove_load_group(i),
                DeleteSetupEntry::DistributedLoad(i) => setup.remove_distributed_load(i),
                DeleteSetupEntry::Material(i) => setup.remove_material(i),
                DeleteSetupEntry::Section(i) => setup.remove_section(i),
            }
        }

        *bg = BackgroundColor(match *interaction {
            Interaction::Pressed | Interaction::Hovered => Color::srgba(0.75, 0.22, 0.22, 0.95),
            Interaction::None => Color::srgba(0.55, 0.18, 0.18, 0.85),
        });
    }
}

pub(crate) fn rebuild_materials_sections_list(
    mut commands: Commands,
    setup: Res<fem_core::AnalysisSetup>,
    container_query: Query<Entity, With<MaterialsSectionsListContainer>>,
    children_query: Query<&Children>,
) {
    if !setup.is_changed() {
        return;
    }

    let Ok(container) = container_query.single() else {
        return;
    };

    if let Ok(children) = children_query.get(container) {
        for &child in children {
            commands.entity(child).despawn();
        }
    }

    commands.entity(container).with_children(|list| {
        for (index, material) in setup.materials.iter().enumerate() {
            setup_entry_row(
                list,
                &format_material_line(material),
                DeleteSetupEntry::Material(index),
                &format!("MaterialLine_{}", material.name),
            );
        }

        for (index, section) in setup.sections.iter().enumerate() {
            setup_entry_row(
                list,
                &format_section_line(section),
                DeleteSetupEntry::Section(index),
                &format!("SectionLine_{}", section.name),
            );
        }
    });
}

/// Spawns one removable list entry: a text line plus a small "✕" button
/// tagged with [`DeleteSetupEntry`], used by both
/// [`rebuild_materials_sections_list`] and (for boundary
/// conditions/loads) the constraint/load panels.
fn setup_entry_row(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    delete_entry: DeleteSetupEntry,
    name: &str,
) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6.0),
                ..default()
            },
            Name::new(name.to_string()),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(label.to_string()),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            row.spawn((
                Button,
                Node {
                    width: px(16.0),
                    height: px(16.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(px(3.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.55, 0.18, 0.18, 0.85)),
                delete_entry,
                Name::new(format!("{name}_DeleteButton")),
            ))
            .with_child((
                Text::new("x"),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(Color::srgb(0.95, 0.85, 0.85)),
            ));
        });
}

/// Formats one [`fem_core::FemMaterial`] as `"[Mat] name  E=.. nu=.. rho=.."`,
/// omitting any property that wasn't present in the source file rather
/// than showing a misleading placeholder value.
fn format_material_line(material: &fem_core::FemMaterial) -> String {
    let mut parts = Vec::new();

    if let Some(young) = material.young_modulus {
        parts.push(format!("E={young:.3e}"));
    }
    if let Some(poisson) = material.poisson_ratio {
        parts.push(format!("nu={poisson:.3}"));
    }
    if let Some(density) = material.density {
        parts.push(format!("rho={density:.3e}"));
    }

    if parts.is_empty() {
        format!("[Mat] {}", material.name)
    } else {
        format!("[Mat] {}  {}", material.name, parts.join(" "))
    }
}

/// Formats one [`fem_core::Section`] as
/// `"[Sec] name  Shell t=2.0  mat=STEEL  (EGRP)"`, varying the
/// geometry field by [`fem_core::SectionKind`].
fn format_section_line(section: &fem_core::Section) -> String {
    let kind_label = match section.kind {
        fem_core::SectionKind::Solid => "Solid".to_string(),
        fem_core::SectionKind::Shell { thickness } => format!("Shell t={thickness:.3}"),
        fem_core::SectionKind::Beam { area } => format!("Beam A={area:.3}"),
    };

    let scope = section
        .element_set_name
        .as_deref()
        .map(|name| format!("  ({name})"))
        .unwrap_or_default();

    format!(
        "[Sec] {}  {kind_label}  mat={}{scope}",
        section.name, section.material_name,
    )
}

fn set_list_button(parent: &mut ChildSpawnerCommands, set_button: SetButton, label: &str) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(px(8.0), px(4.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            set_button,
            Name::new(format!("SetButton_{label}")),
        ))
        .with_child((
            Text::new(label.to_string()),
            TextFont {
                font_size: FontSize::Px(10.5),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));
}

/// Selects every member of the clicked set, mirroring the result of a box
/// select over exactly that set: applies the shared selection modifier,
/// sets [`SelectionFilter::level`] to match the set's
/// kind, and marks matching [`Selectable`] entities as [`Selected`] so
/// per-entity rendering (small meshes) highlights them too, alongside the
/// [`SelectionState::targets`] used by the topology highlight overlay for
/// large/aggregate meshes.
pub(crate) fn set_button_system(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    model: Option<Res<FemModel>>,
    mut filter: ResMut<SelectionFilter>,
    mut selection: ResMut<SelectionState>,
    selectable_query: Query<(Entity, &Selectable)>,
    selected_query: Query<Entity, With<Selected>>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &SetButton,
        ),
        With<SetButton>,
    >,
) {
    for (interaction, mut background, mut border, set_button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(model) = model.as_deref() {
                if let Some(mesh) = model.meshes.get(set_button.mesh_index) {
                    let targets = match set_button.kind {
                        SetKind::Node => mesh
                            .node_sets
                            .get(set_button.set_index)
                            .map(|set| mesh.node_set_targets(set)),
                        SetKind::Element => mesh
                            .element_sets
                            .get(set_button.set_index)
                            .map(|set| mesh.element_set_targets(set)),
                        SetKind::Surface => mesh
                            .surface_sets
                            .get(set_button.set_index)
                            .map(|set| mesh.surface_set_targets(set)),
                    };

                    if let Some(local_targets) = targets {
                        let targets: Vec<fem_core::FemEntityRef> = local_targets
                            .into_iter()
                            .map(|target| {
                                fem_core::FemEntityRef::new(set_button.mesh_index, target)
                            })
                            .collect();
                        let ctrl = keyboard.pressed(KeyCode::ControlLeft)
                            || keyboard.pressed(KeyCode::ControlRight);
                        let shift = keyboard.pressed(KeyCode::ShiftLeft)
                            || keyboard.pressed(KeyCode::ShiftRight);
                        let alt = keyboard.pressed(KeyCode::AltLeft)
                            || keyboard.pressed(KeyCode::AltRight);
                        let operation =
                            selection::SelectionOperation::from_modifiers(ctrl, shift, alt);

                        filter.level = match set_button.kind {
                            SetKind::Node => SelectionLevel::Node,
                            SetKind::Element => SelectionLevel::Element,
                            SetKind::Surface => SelectionLevel::Face,
                        };

                        selection.apply_group(&targets, &targets, operation);

                        for entity in &selected_query {
                            commands.entity(entity).remove::<Selected>();
                        }
                        selection.entities.clear();

                        for (entity, selectable) in &selectable_query {
                            if selection.targets.contains(&selectable.target) {
                                commands.entity(entity).insert(Selected);
                                selection.entities.push(entity);
                            }
                        }
                    }
                }
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn update_mesh_stats_text(
    model: Option<Res<FemModel>>,
    status: Res<MeshLoadStatus>,
    settings: Res<VisualizationSettings>,
    mut query: Query<&mut Text, With<MeshStatsText>>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };

    let Some(model) = model else {
        **text = "Mesh: demo pending".to_string();
        return;
    };

    let mesh_count = model.meshes.len();
    let node_count: usize = model.meshes.iter().map(|mesh| mesh.nodes.len()).sum();
    let element_count: usize = model.meshes.iter().map(|mesh| mesh.elements.len()).sum();
    let edge_count: usize = model
        .meshes
        .iter()
        .map(|mesh| mesh.cached_edges().len())
        .sum();
    let face_count: usize = model
        .meshes
        .iter()
        .map(|mesh| mesh.cached_boundary_faces().len())
        .sum();
    let boundary_edge_count: usize = model
        .meshes
        .iter()
        .map(|mesh| mesh.cached_boundary_edges().len())
        .sum();
    let node_set_count: usize = model.meshes.iter().map(|mesh| mesh.node_sets.len()).sum();
    let element_set_count: usize = model
        .meshes
        .iter()
        .map(|mesh| mesh.element_sets.len())
        .sum();
    let surface_set_count: usize = model
        .meshes
        .iter()
        .map(|mesh| mesh.surface_sets.len())
        .sum();
    let contact_count = model.contacts.len();
    let load_status = load_status_line(&status);

    **text = format!(
        "{load_status}\nMeshes: {mesh_count}  Nodes: {node_count}  Elements: {element_count}\nEdges: {edge_count}  Boundary edges: {boundary_edge_count}\nBoundary faces: {face_count}  Render: {}\nSets: N {node_set_count}  E {element_set_count}  S {surface_set_count}  Contacts: {contact_count}",
        settings.mode.label()
    );
}

/// Rebuilds the assembly part picker when the imported part list changes.
/// Coordinate edits do not rebuild it because its signature contains only
/// names and mesh counts, avoiding button churn during repeated nudges.
pub(crate) fn rebuild_assembly_parts(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    mut state: ResMut<AssemblyEditorState>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut last_signature: Local<Vec<(String, usize, usize)>>,
    container_query: Query<Entity, With<AssemblyPartsContainer>>,
    children_query: Query<&Children>,
) {
    let signature: Vec<_> = model
        .as_deref()
        .map(|model| {
            model
                .parts
                .iter()
                .map(|part| {
                    let (nodes, elements) = model
                        .meshes
                        .get(part.mesh_index)
                        .map(|mesh| (mesh.nodes.len(), mesh.elements.len()))
                        .unwrap_or_default();
                    (part.name.clone(), nodes, elements)
                })
                .collect()
        })
        .unwrap_or_default();

    if *last_signature == signature {
        return;
    }
    *last_signature = signature.clone();
    measurement.clear();

    state.selected_part = match (state.selected_part, signature.len()) {
        (_, 0) => None,
        (Some(index), len) if index < len => Some(index),
        _ => Some(0),
    };

    let Ok(container) = container_query.single() else {
        return;
    };
    if let Ok(children) = children_query.get(container) {
        for &child in children {
            commands.entity(child).despawn();
        }
    }

    commands.entity(container).with_children(|list| {
        for (part_index, (name, nodes, elements)) in signature.iter().enumerate() {
            let label = format!(
                "[{}] {}   {} N / {} E",
                part_index + 1,
                name,
                nodes,
                elements
            );
            list.spawn((
                Button,
                Node {
                    width: percent(100.0),
                    padding: UiRect::axes(px(8.0), px(5.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(4.0)),
                    ..default()
                },
                BackgroundColor(BUTTON_NORMAL),
                BorderColor::all(PANEL_BORDER),
                AssemblyPartButton { part_index },
                Name::new(format!("AssemblyPartButton_{part_index}")),
            ))
            .with_child((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(10.5),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));
        }
    });
}

pub(crate) fn assembly_gizmo_mode_button_system(
    mut state: ResMut<AssemblyEditorState>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut buttons: Query<(
        Ref<Interaction>,
        &AssemblyGizmoModeButton,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (interaction, button, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            state.gizmo_mode = button.mode;
            state.hovered_axis = None;
            state.hovered_part = None;
            measurement.clear();
        }

        let active = state.gizmo_mode == button.mode;
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });
    }
}

pub(crate) fn assembly_part_button_system(
    mut state: ResMut<AssemblyEditorState>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut buttons: Query<(
        Ref<Interaction>,
        &AssemblyPartButton,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (interaction, button, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            state.selected_part = Some(button.part_index);
            measurement.clear();
        }

        let active = state.selected_part == Some(button.part_index);
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });
    }
}

pub(crate) fn assembly_mode_button_system(
    mut commands: Commands,
    mut tool: ResMut<ViewportTool>,
    mut state: ResMut<AssemblyEditorState>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut hover: ResMut<HoverResult>,
    mut selection: ResMut<SelectionState>,
    selected_query: Query<Entity, With<Selected>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<AssemblyModeButton>,
    >,
    mut labels: Query<&mut Text, With<AssemblyModeButtonLabel>>,
) {
    let Ok((interaction, mut background, mut border)) = buttons.single_mut() else {
        return;
    };

    if *interaction == Interaction::Pressed && interaction.is_changed() {
        *tool = match *tool {
            ViewportTool::Selection => ViewportTool::Assembly,
            ViewportTool::Assembly => ViewportTool::Selection,
        };
        state.hovered_part = None;
        state.hovered_axis = None;
        measurement.clear();

        if *tool == ViewportTool::Assembly {
            hover.clear();
            selection.clear();
            for entity in &selected_query {
                commands.entity(entity).remove::<Selected>();
            }
        }
    }

    let active = *tool == ViewportTool::Assembly;
    *background = BackgroundColor(match (*interaction, active) {
        (Interaction::Pressed, _) => BUTTON_PRESSED,
        (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
        (Interaction::Hovered, false) => BUTTON_HOVERED,
        (Interaction::None, false) => BUTTON_NORMAL,
    });
    *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });

    if let Ok(mut label) = labels.single_mut() {
        **label = if active {
            "Edit in viewport: ON"
        } else {
            "Edit in viewport: OFF"
        }
        .to_string();
    }
}

fn assembly_slider_value(
    sliders: &Query<&SliderState, With<SliderTrack>>,
    id: SliderId,
    default_value: f32,
) -> f32 {
    sliders
        .iter()
        .find(|state| state.id == id)
        .map(|state| state.value)
        .unwrap_or(default_value)
}

pub(crate) fn assembly_transform_button_system(
    mut model: ResMut<FemModel>,
    state: Res<AssemblyEditorState>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut version: ResMut<FemModelVersion>,
    mut contact_candidates: ResMut<ContactCandidateState>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut buttons: Query<(
        Ref<Interaction>,
        &AssemblyTransformButton,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (interaction, button, mut background, mut border) in &mut buttons {
        let mut changed = false;
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            measurement.clear();
            if let Some(part_index) = state.selected_part {
                changed = match button.action {
                    AssemblyTransformAction::Translate(direction) => {
                        let percent =
                            assembly_slider_value(&sliders, SliderId::AssemblyMovePercent, 1.0);
                        let step = assembly_reference_size(&model, part_index) * percent / 100.0;
                        model.translate_part(part_index, direction * step)
                    }
                    AssemblyTransformAction::Rotate(axis) => {
                        let degrees =
                            assembly_slider_value(&sliders, SliderId::AssemblyRotationDegrees, 5.0);
                        model.rotate_part_about_centroid(
                            part_index,
                            Quat::from_axis_angle(axis.normalize(), degrees.to_radians()),
                        )
                    }
                    AssemblyTransformAction::Reset => model.reset_part_pose(part_index),
                };
            }
        }

        if changed {
            contact_candidates.candidates.clear();
            contact_candidates.selected = None;
            version.bump();
        }

        *background = BackgroundColor(match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        });
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn update_assembly_status_text(
    model: Res<FemModel>,
    state: Res<AssemblyEditorState>,
    tool: Res<ViewportTool>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut query: Query<&mut Text, With<AssemblyStatusText>>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };
    let Some(part_index) = state.selected_part else {
        **text = "No part selected".to_string();
        return;
    };
    let Some(part) = model.parts.get(part_index) else {
        **text = "No part selected".to_string();
        return;
    };

    let center = model.part_centroid(part_index).unwrap_or(Vec3::ZERO);
    let percent = assembly_slider_value(&sliders, SliderId::AssemblyMovePercent, 1.0);
    let step = assembly_reference_size(&model, part_index) * percent / 100.0;
    let viewport_hint = if *tool == ViewportTool::Assembly {
        match state.gizmo_mode {
            AssemblyGizmoMode::Move => "Viewport Move: drag X/Y/Z arrow   Shift=fine Ctrl=snap",
            AssemblyGizmoMode::Rotate => {
                "Viewport Rotate: drag RX/RY/RZ ring   Shift=fine Ctrl=snap"
            }
        }
    } else {
        "Viewport edit is OFF; panel nudges remain available"
    };
    **text = format!(
        "Selected: {}\nCenter: ({:.4}, {:.4}, {:.4})   Move: {:.4}\n{}",
        part.name, center.x, center.y, center.z, step, viewport_hint,
    );
}

pub(crate) fn update_selection_stats_text(
    filter: Res<SelectionFilter>,
    selection: Res<SelectionState>,
    hover: Res<HoverResult>,
    model: Option<Res<FemModel>>,
    mut query: Query<&mut Text, With<SelectionStatsText>>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };

    let hover_text = hover
        .target()
        .map(|target| entity_label(target, model.as_deref()))
        .unwrap_or("none".to_string());

    **text = format!(
        "Filter: {}  Selected: {}  Hover: {}",
        selection_level_label(filter.level),
        selection.len(),
        hover_text
    );
}

pub(crate) fn update_contact_candidate_text(
    state: Res<ContactCandidateState>,
    model: Option<Res<FemModel>>,
    mut query: Query<&mut Text, With<ContactCandidateText>>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };

    **text = contact_candidate_summary(&state, model.as_deref());
}

pub(crate) fn rebuild_contact_definitions_list(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    version: Res<FemModelVersion>,
    mut last_version: Local<Option<u64>>,
    container_query: Query<Entity, With<ContactDefinitionsListContainer>>,
    children_query: Query<&Children>,
    mut summary_query: Query<&mut Text, With<ContactDefinitionsText>>,
    mut preview: ResMut<DefinedContactPreview>,
) {
    let current = version.value;
    let model_changed = model.as_ref().is_some_and(|model| model.is_changed());
    if *last_version == Some(current) && !model_changed {
        return;
    }
    *last_version = Some(current);

    let Ok(container) = container_query.single() else {
        return;
    };

    if let Ok(children) = children_query.get(container) {
        for &child in children {
            commands.entity(child).despawn();
        }
    }

    let contact_count = model.as_deref().map_or(0, |model| model.contacts.len());
    if let Ok(mut summary) = summary_query.single_mut() {
        **summary = format!("Defined contacts: {contact_count}");
    }

    let next_selected = match preview.selected {
        Some(index) if index < contact_count => Some(index),
        _ if contact_count > 0 => Some(0),
        _ => None,
    };
    if preview.selected != next_selected {
        preview.selected = next_selected;
    }
    if contact_count == 0 && preview.active {
        preview.active = false;
    }

    let Some(model) = model.as_deref() else {
        return;
    };
    commands.entity(container).with_children(|list| {
        for (index, contact) in model.contacts.iter().enumerate() {
            let slave = model.contact_slave_name(contact.slave).unwrap_or("?");
            let master = model.surface_set_name(contact.master).unwrap_or("?");
            let pair_kind = match contact.slave {
                ContactSlaveRef::Nodes(_) => "NODE-SURF",
                ContactSlaveRef::Surface(_) => "SURF-SURF",
            };
            let parameters = match contact.contact_type {
                ContactType::Tied => String::new(),
                ContactType::SmallSliding | ContactType::FiniteSliding => {
                    let penalty = contact
                        .penalty_factor
                        .map(|factor| format!(" | penalty={factor:.3e}"))
                        .unwrap_or_default();
                    format!(" | mu={:.4}{penalty}", contact.friction_coefficient)
                }
            };
            contact_definition_button(
                list,
                index,
                &format!(
                    "[{}] {} | {}{}\n{} -> {}",
                    contact.name,
                    contact.contact_type.label(),
                    pair_kind,
                    parameters,
                    slave,
                    master,
                ),
            );
        }
    });
}

fn contact_definition_button(parent: &mut ChildSpawnerCommands, index: usize, label: &str) {
    parent
        .spawn((
            Button,
            Node {
                width: percent(100.0),
                min_height: px(42.0),
                padding: UiRect::axes(px(8.0), px(5.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            DefinedContactButton(index),
            Name::new(format!("DefinedContact_{index}")),
        ))
        .with_child((
            Text::new(label.to_string()),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));
}

pub(crate) fn defined_contact_button_system(
    mut preview: ResMut<DefinedContactPreview>,
    mut draft: ResMut<ContactDraftPreview>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &DefinedContactButton,
        ),
        With<DefinedContactButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            preview.selected = Some(button.0);
            draft.clear();
        }

        let selected = preview.selected == Some(button.0);
        let color = match (*interaction, selected) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *background = BackgroundColor(color);
        *border = BorderColor::all(if selected {
            ACTIVE_BORDER
        } else {
            PANEL_BORDER
        });
    }
}

/// Enables contact overlays only on the Contact page. Candidate review has
/// first priority, an in-progress draft second, and a defined pair third.
pub(crate) fn sync_defined_contact_preview(
    page: Res<SidebarPage>,
    model: Option<Res<FemModel>>,
    candidates: Res<ContactCandidateState>,
    mut preview: ResMut<DefinedContactPreview>,
    mut draft: ResMut<ContactDraftPreview>,
) {
    let contact_count = model.as_deref().map_or(0, |model| model.contacts.len());
    let selected = match preview.selected {
        Some(index) if index < contact_count => Some(index),
        _ if contact_count > 0 => Some(0),
        _ => None,
    };
    let draft_has_geometry = draft.master.is_some() || draft.slave.is_some();
    let draft_active = *page == SidebarPage::Contact
        && candidates.selected_candidate().is_none()
        && draft_has_geometry;
    let active = *page == SidebarPage::Contact
        && candidates.selected_candidate().is_none()
        && !draft_has_geometry
        && selected.is_some();

    if preview.selected != selected {
        preview.selected = selected;
    }
    if preview.active != active {
        preview.active = active;
    }
    if draft.active != draft_active {
        draft.active = draft_active;
    }
}

pub(crate) fn update_contact_review_controls(
    state: Res<ContactCandidateState>,
    mut controls: Query<&mut Node, With<ContactReviewControls>>,
) {
    if !state.is_changed() {
        return;
    }

    let display = if state.selected_candidate().is_some() {
        Display::Flex
    } else {
        Display::None
    };

    for mut node in &mut controls {
        node.display = display;
    }
}

fn contact_candidate_summary(state: &ContactCandidateState, model: Option<&FemModel>) -> String {
    let total = state.candidates.len();

    let Some(candidate) = state.selected_candidate() else {
        return if total == 0 {
            "No candidates — run Detect Contact Candidates".to_string()
        } else {
            format!("Contacts: {total} candidates")
        };
    };

    let selected_index = state.selected.unwrap_or(0);
    let mesh_a = mesh_label(model, candidate.mesh_a);
    let mesh_b = mesh_label(model, candidate.mesh_b);
    let kind = if candidate.is_self_contact() {
        "self"
    } else {
        "cross-part"
    };

    format!(
        "Contact candidate {}/{total} ({kind})\n{mesh_a} <-> {mesh_b}\nFaces: {} / {}  Pairs: {}  Avg gap: {:.4}",
        selected_index + 1,
        candidate.faces_a.len(),
        candidate.faces_b.len(),
        candidate.pair_count,
        candidate.average_gap,
    )
}

fn mesh_label(model: Option<&FemModel>, mesh_index: usize) -> String {
    model
        .and_then(|model| {
            model
                .parts
                .iter()
                .find(|part| part.mesh_index == mesh_index)
        })
        .map(|part| part.name.clone())
        .unwrap_or_else(|| format!("Mesh {mesh_index}"))
}

// ── post-process result systems ───────────────────────────────────────────────

/// Opens a file dialog for FrontISTR result files (`.res.0.*`), stores the
/// chosen path in a `Local<Option<PathBuf>>` shared with
/// [`result_load_system`].
pub(crate) fn open_result_button_system(
    mut pending_path: Local<Option<std::path::PathBuf>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<OpenResultButton>,
    >,
    model: Option<Res<FemModel>>,
    mut results: ResMut<FemResultSet>,
    mut settings: ResMut<visualization::VisualizationSettings>,
    mut page: ResMut<SidebarPage>,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Open result file")
                .add_filter("All result files", &["res", "frd", "vtu", "pvtu"])
                .add_filter("FrontISTR result (.res.0.*)", &["res"])
                .add_filter("CalculiX result (.frd)", &["frd"])
                .add_filter("VTK XML (.vtu / .pvtu)", &["vtu", "pvtu"])
                .pick_file()
            {
                *pending_path = Some(path);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }

    // Load on a separate branch to avoid holding rfd dialog open
    // while mutating FemResultSet.
    if let Some(path) = pending_path.take() {
        let Some(model) = model.as_deref() else {
            return;
        };
        let Some(fem_mesh) = model.meshes.first() else {
            return;
        };

        let node_ids: Vec<fem_core::NodeId> = fem_mesh.nodes.iter().map(|n| n.id).collect();

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        // Ensure by_mesh slot exists for mesh 0.
        if results.by_mesh.is_empty() {
            results.by_mesh.push(Vec::new());
        }

        let loaded_steps: Vec<fem_core::StepResult> = match ext.as_str() {
            "frd" => match hecmw::load_frd_file(&path, &node_ids) {
                Ok(steps) => steps,
                Err(err) => {
                    bevy::log::warn!("FRD load failed: {err}");
                    return;
                }
            },
            "vtu" | "pvtu" => match hecmw::load_vtu_file(&path, &node_ids) {
                Ok(step) => vec![step],
                Err(err) => {
                    bevy::log::warn!("VTU load failed: {err}");
                    return;
                }
            },
            _ => {
                // .res.0.N — auto-detect series siblings and load all steps.
                match hecmw::load_series(&path, &node_ids) {
                    Ok(steps) => steps,
                    Err(err) => {
                        bevy::log::warn!("Result series load failed: {err}");
                        return;
                    }
                }
            }
        };

        if loaded_steps.is_empty() {
            bevy::log::warn!("Result file contained no steps: {:?}", path.file_name());
            return;
        }

        let step_count = loaded_steps.len();
        results.by_mesh[0].extend(loaded_steps);
        results.activate_first();

        // Auto-activate contour.
        if let Some(active) = &results.active {
            let has_disp = results
                .by_mesh
                .get(active.mesh_index)
                .and_then(|s| s.get(active.step_index))
                .map(|s| s.field_by_name("Displacement").is_some())
                .unwrap_or(false);

            settings.contour = Some(ContourSettings {
                mesh_index: active.mesh_index,
                step_index: active.step_index,
                field_name: active.field_name.clone(),
                show_deformation: has_disp,
                displacement_field: "Displacement".to_string(),
                deformation_scale: 1.0,
            });
        }

        bevy::log::info!(
            "Loaded {step_count} result step(s) from {:?}",
            path.file_name()
        );
        // A newly loaded result is immediately visible without another
        // navigation click.
        *page = SidebarPage::Results;
    }
}

// ── analysis setup (boundary conditions / loads / materials) systems ───────────

/// Handles the ▶ Export button: opens a folder-picker dialog, then writes
/// `hecmw_ctrl.dat`, `<stem>.msh`, and `<stem>.cnt` to the chosen directory.
// ── project open ──────────────────────────────────────────────────────────────

/// Opens `hecmw_ctrl.dat`, reads mesh + cnt file names, and loads both in
/// one click — so a person with an existing FrontISTR job folder can start
/// working in under two seconds.
pub(crate) fn open_project_button_system(
    mut request: ResMut<MeshLoadRequest>,
    mut load_status: ResMut<MeshLoadStatus>,
    mut pending_cnt: ResMut<fem_core::PendingCntLoad>,
    version: Res<FemModelVersion>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<OpenProjectButton>,
    >,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let Some(ctrl_path) = rfd::FileDialog::new()
                .set_title("Open FrontISTR project (hecmw_ctrl.dat)")
                .add_filter("FrontISTR project", &["dat"])
                .add_filter("All files", &["*"])
                .pick_file()
            else {
                continue;
            };

            match hecmw::load_hecmw_ctrl(&ctrl_path) {
                Ok(content) => {
                    let (mesh_path, cnt_path) = hecmw::resolve_paths(&ctrl_path, &content);

                    if let Some(mesh_path) = mesh_path {
                        load_status.loading(mesh_path.clone());
                        request.request(mesh_path);
                    } else {
                        bevy::log::warn!("hecmw_ctrl.dat: mesh file not found");
                    }

                    // Queue the cnt to load after the mesh: `mesh_load_system`
                    // consumes `request` on its own turn (possibly a later
                    // frame than this one), so the mesh this cnt's node/
                    // element groups need to resolve against doesn't exist
                    // yet. `PendingCntLoad` records the current model
                    // version and only applies once
                    // `apply_pending_cnt_system` sees it advance — see
                    // `PendingCntLoad`'s doc comment for why a naive
                    // "read the model now" approach would race the load.
                    if let Some(cnt_path) = cnt_path {
                        pending_cnt.request(cnt_path, 0, version.value);
                    }
                }
                Err(e) => bevy::log::warn!("Failed to parse hecmw_ctrl.dat: {e}"),
            }
        }

        let color = match *interaction {
            Interaction::Pressed => Color::srgb(0.12, 0.44, 0.22),
            Interaction::Hovered => Color::srgb(0.14, 0.52, 0.26),
            Interaction::None => Color::srgb(0.10, 0.30, 0.18),
        };

        *bg = BackgroundColor(color);
        *border = BorderColor::all(Color::srgb(0.15, 0.46, 0.26));
    }
}

/// Applies a `.cnt` file queued by [`open_project_button_system`] once its
/// paired mesh load has actually completed. Runs `.after(mesh_load_system)`
/// so a mesh that finishes loading this same frame is picked up on the very
/// next frame rather than waiting an extra one.
pub(crate) fn apply_pending_cnt_system(
    mut model: Option<ResMut<FemModel>>,
    version: Res<FemModelVersion>,
    mut pending_cnt: ResMut<fem_core::PendingCntLoad>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
) {
    if pending_cnt.path.is_none() {
        return;
    }

    let Some((path, mesh_index)) = pending_cnt.take_if_ready(version.value) else {
        return;
    };
    let Some(model) = model.as_deref_mut() else {
        return;
    };
    let Some(mesh) = model.meshes.get(mesh_index) else {
        return;
    };

    match hecmw::load_cnt_file(&path, mesh, mesh_index) {
        Ok(data) => {
            let counts = (
                data.boundary_conditions.len(),
                data.boundary_conditions
                    .iter()
                    .map(|condition| condition.nodes.len())
                    .sum::<usize>(),
                data.nodal_loads.len(),
                data.distributed_loads.len(),
                data.materials.len(),
                data.sections.len(),
                data.contact_settings.len(),
            );
            let applied_contacts = data.apply_contact_settings(&mut model.contacts);
            data.merge_into(&mut setup);
            setup.set_changed();
            bevy::log::info!(
                "Loaded analysis setup from {:?}: {} BCs / {} constrained nodes, {} nodal loads, {} distributed loads, {} materials, {} sections, {applied_contacts}/{} contacts",
                path.file_name(),
                counts.0,
                counts.1,
                counts.2,
                counts.3,
                counts.4,
                counts.5,
                counts.6,
            );
        }
        Err(e) => bevy::log::warn!("Failed to parse cnt file {:?}: {e}", path),
    }
}

// ── group creation ────────────────────────────────────────────────────────────

/// Saves the currently selected nodes as a new [`fem_core::FemNodeSet`]
/// (NGRP) named `NGRP1`, `NGRP2`, … .
///
/// Uses `ResMut<FemModel>` so that Bevy's change detection fires and
/// [`rebuild_sets_list`] rebuilds the SETS panel automatically.
pub(crate) fn make_node_group_button_system(
    mut model: ResMut<FemModel>,
    selection: Res<SelectionState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<MakeNodeGroupButton>,
    >,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            for (mesh_index, nodes) in selected_nodes_by_mesh(&selection) {
                let Some(mesh) = model.meshes.get_mut(mesh_index) else {
                    continue;
                };
                if nodes.is_empty() {
                    continue;
                }

                let n = mesh.node_sets.len() + 1;
                let name = format!("NGRP{n}");
                mesh.node_sets.push(fem_core::FemNodeSet { name, nodes });
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Saves the currently selected elements as a new [`fem_core::FemElementSet`]
/// (EGRP) named `EGRP1`, `EGRP2`, … .
pub(crate) fn make_element_group_button_system(
    mut model: ResMut<FemModel>,
    selection: Res<SelectionState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<MakeElementGroupButton>,
    >,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let mut by_mesh = BTreeMap::<usize, BTreeSet<fem_core::ElementId>>::new();
            for target in &selection.targets {
                if let FemEntityId::Element(id) = target.entity {
                    by_mesh.entry(target.mesh_index).or_default().insert(id);
                }
            }

            for (mesh_index, elements) in by_mesh {
                let Some(mesh) = model.meshes.get_mut(mesh_index) else {
                    continue;
                };
                if elements.is_empty() {
                    continue;
                }

                let n = mesh.element_sets.len() + 1;
                let name = format!("EGRP{n}");
                mesh.element_sets.push(fem_core::FemElementSet {
                    name,
                    elements: elements.into_iter().collect(),
                });
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

// ── section definition systems ────────────────────────────────────────────────

/// Toggles the active section type when a [Solid]/[Shell]/[Beam] button
/// is clicked.
pub(crate) fn section_type_button_system(
    mut selected: ResMut<SelectedSectionType>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &SectionTypeButton,
        ),
        With<SectionTypeButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            *selected = btn.0;
        }

        let active = *selected == btn.0;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Selects an EGRP for the section definition panel.
pub(crate) fn egrp_select_button_system(
    mut selected: ResMut<SelectedEgrp>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &EgrpSelectButton,
        ),
        With<EgrpSelectButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            selected.0 = btn.0.clone();
        }

        let active = selected.0 == btn.0;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Selects a material for the section definition panel.
pub(crate) fn material_select_button_system(
    mut selected: ResMut<SelectedMaterialForSection>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &MaterialSelectButton,
        ),
        With<MaterialSelectButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            selected.0 = Some(btn.0.clone());
        }

        let active = selected.0.as_deref() == Some(&btn.0);
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Applies the configured section to [`fem_core::AnalysisSetup`].
pub(crate) fn add_section_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    section_type: Res<SelectedSectionType>,
    egrp: Res<SelectedEgrp>,
    material_sel: Res<SelectedMaterialForSection>,
    slider_query: Query<&SliderState, With<SliderTrack>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<AddSectionButton>,
    >,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let Some(mat_name) = &material_sel.0 else {
                continue;
            };

            let thickness = slider_query
                .iter()
                .find(|s| s.id == SliderId::SectionThickness)
                .map(|s| s.value)
                .unwrap_or(2.0);

            let kind = match *section_type {
                SelectedSectionType::Solid => fem_core::SectionKind::Solid,
                SelectedSectionType::Shell => fem_core::SectionKind::Shell { thickness },
                SelectedSectionType::Beam => fem_core::SectionKind::Beam { area: thickness },
            };

            setup.add_section(0, mat_name.clone(), egrp.0.clone(), kind);
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Rebuilds the dynamic EGRP and material selector rows in the section
/// definition panel whenever the model or setup changes.
pub(crate) fn rebuild_section_def_panel(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    setup: Res<fem_core::AnalysisSetup>,
    version: Res<FemModelVersion>,
    mut last_ver: Local<Option<u64>>,
    egrp_row_q: Query<Entity, With<SectionDefEgrpRow>>,
    mat_row_q: Query<Entity, With<SectionDefMatRow>>,
    children_q: Query<&Children>,
) {
    let ver_changed = *last_ver != Some(version.value);
    *last_ver = Some(version.value);

    if !ver_changed && !setup.is_changed() {
        return;
    }

    // ── EGRP buttons ──
    if let Ok(egrp_row) = egrp_row_q.single() {
        if let Ok(children) = children_q.get(egrp_row) {
            for &c in children {
                commands.entity(c).despawn();
            }
        }

        commands.entity(egrp_row).with_children(|row| {
            // "ALL" option
            row.spawn((
                Button,
                Node {
                    padding: UiRect::axes(px(8.0), px(3.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(4.0)),
                    ..default()
                },
                BackgroundColor(BUTTON_NORMAL),
                BorderColor::all(PANEL_BORDER),
                EgrpSelectButton(None),
                Name::new("Egrp_ALL"),
            ))
            .with_child((
                Text::new("ALL"),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));

            if let Some(model) = model.as_deref() {
                for mesh in &model.meshes {
                    for eset in &mesh.element_sets {
                        let name = eset.name.clone();
                        row.spawn((
                            Button,
                            Node {
                                padding: UiRect::axes(px(8.0), px(3.0)),
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            EgrpSelectButton(Some(name.clone())),
                            Name::new(format!("Egrp_{name}")),
                        ))
                        .with_child((
                            Text::new(name),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    }
                }
            }
        });
    }

    // ── Material buttons ──
    if let Ok(mat_row) = mat_row_q.single() {
        if let Ok(children) = children_q.get(mat_row) {
            for &c in children {
                commands.entity(c).despawn();
            }
        }

        commands.entity(mat_row).with_children(|row| {
            for mat in &setup.materials {
                let name = mat.name.clone();
                row.spawn((
                    Button,
                    Node {
                        padding: UiRect::axes(px(8.0), px(3.0)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(4.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(PANEL_BORDER),
                    MaterialSelectButton(name.clone()),
                    Name::new(format!("MatSel_{name}")),
                ))
                .with_child((
                    Text::new(name),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));
            }
        });
    }
}

pub(crate) fn export_button_system(
    model: Option<Res<FemModel>>,
    status: Res<MeshLoadStatus>,
    setup: Res<fem_core::AnalysisSetup>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ExportButton>,
    >,
    mut status_query: Query<&mut Text, With<ExportStatusText>>,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let Some(model) = model.as_deref() else {
                set_export_status(&mut status_query, "Error: no mesh loaded");
                continue;
            };

            let stem = status
                .last_path
                .as_deref()
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .unwrap_or("mesh")
                .to_string();

            let Some(dir) = rfd::FileDialog::new()
                .set_title("Export FrontISTR files to folder")
                .pick_folder()
            else {
                continue;
            };

            match hecmw::write_frontistr_project(&dir, &stem, model, &setup) {
                Ok(summary) => {
                    let part_note = if summary.part_count > 1 {
                        format!("  ({} parts merged)", summary.part_count)
                    } else {
                        String::new()
                    };
                    let message = format!(
                        "OK {stem}.*{part_note}\n{}N/{}E  BC:{} Ld:{} Mat:{} Sec:{} Ctc:{}",
                        summary.node_count,
                        summary.element_count,
                        summary.boundary_condition_count,
                        summary.load_count,
                        summary.material_count,
                        summary.section_count,
                        summary.contact_count,
                    );
                    set_export_status(&mut status_query, &message);
                }
                Err(e) => set_export_status(&mut status_query, &format!("Error: {e}")),
            }
        }

        let color = match *interaction {
            Interaction::Pressed => Color::srgb(0.12, 0.44, 0.22),
            Interaction::Hovered => Color::srgb(0.14, 0.52, 0.26),
            Interaction::None => Color::srgb(0.10, 0.32, 0.18),
        };

        *bg = BackgroundColor(color);
        *border = BorderColor::all(Color::srgb(0.15, 0.50, 0.28));
    }
}

fn set_export_status(query: &mut Query<&mut Text, With<ExportStatusText>>, msg: &str) {
    if let Ok(mut text) = query.single_mut() {
        **text = msg.to_string();
    }
}

/// Opens a file dialog for a FrontISTR `.cnt` analysis control file and
/// merges its boundary conditions / loads / materials / sections into
/// [`fem_core::AnalysisSetup`] for the model's first mesh.
///
/// Like [`open_result_button_system`], this resolves and loads the file in
/// the same system (rather than deferring to `mesh_load_system`'s
/// request/poll pattern) since `.cnt` data doesn't replace the mesh and so
/// doesn't need [`FemModelVersion`] bumped or visuals respawned — only
/// [`AnalysisSetup`]'s change detection, which
/// [`visualization::spawn_boundary_visuals`] already watches.
pub(crate) fn open_setup_button_system(
    mut pending_path: Local<Option<std::path::PathBuf>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<OpenSetupButton>,
    >,
    mut model: Option<ResMut<FemModel>>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Open analysis control file")
                .add_filter("FrontISTR control (.cnt)", &["cnt"])
                .pick_file()
            {
                *pending_path = Some(path);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }

    if let Some(path) = pending_path.take() {
        let Some(model) = model.as_deref_mut() else {
            return;
        };
        let Some(mesh) = model.meshes.first() else {
            return;
        };

        match hecmw::load_cnt_file(&path, mesh, 0) {
            Ok(data) => {
                let applied_contacts = data.apply_contact_settings(&mut model.contacts);
                data.merge_into(&mut setup);

                // Touch the resource so `is_changed()` consumers (e.g.
                // `update_analysis_setup_stats_text`) fire even if every
                // `extend` above happened to add zero items.
                setup.set_changed();

                bevy::log::info!(
                    "Loaded analysis setup from {:?}; updated {applied_contacts} contact pair(s)",
                    path.file_name()
                );
            }
            Err(err) => {
                bevy::log::warn!("Failed to load .cnt file: {err}");
            }
        }
    }
}

/// Toggles [`visualization::BoundaryVisualSettings::show_constraints`].
pub(crate) fn toggle_constraints_button_system(
    mut settings: ResMut<visualization::BoundaryVisualSettings>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ToggleConstraintsButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            settings.show_constraints = !settings.show_constraints;
        }

        let active = settings.show_constraints;

        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::None, false) => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Toggles [`visualization::BoundaryVisualSettings::show_loads`].
pub(crate) fn toggle_loads_button_system(
    mut settings: ResMut<visualization::BoundaryVisualSettings>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ToggleLoadsButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            settings.show_loads = !settings.show_loads;
        }

        let active = settings.show_loads;

        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::None, false) => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Summarizes [`fem_core::AnalysisSetup`] (BC/load/material/section counts)
/// in the panel, whenever it changes.
pub(crate) fn update_analysis_setup_stats_text(
    setup: Res<fem_core::AnalysisSetup>,
    mut query: Query<&mut Text, With<AnalysisSetupStatsText>>,
) {
    if !setup.is_changed() {
        return;
    }

    let Ok(mut text) = query.single_mut() else {
        return;
    };

    **text = if setup.is_empty() {
        "Setup: none loaded".to_string()
    } else {
        let constrained_nodes: usize = setup
            .boundary_conditions
            .iter()
            .map(|bc| bc.nodes.len())
            .sum();

        format!(
            "Setup: BC {} ({} nodes)  Loads {}  Materials {}  Sections {}",
            setup.boundary_conditions.len(),
            constrained_nodes,
            setup.nodal_loads.len() + setup.distributed_loads.len(),
            setup.materials.len(),
            setup.sections.len(),
        )
    };
}

/// Updates the result stats text whenever [`FemResultSet`] changes.
pub(crate) fn update_result_stats_text(
    results: Res<FemResultSet>,
    mut query: Query<&mut Text, With<ResultStatsText>>,
) {
    if !results.is_changed() {
        return;
    }

    let Ok(mut text) = query.single_mut() else {
        return;
    };

    **text = if !results.has_results() {
        "Result: none loaded".to_string()
    } else if let Some(field) = results.active_field() {
        match field {
            fem_core::ResultField::NodeScalar { name, min, max, .. } => {
                format!("Result: {name}\nMin: {min:.4e}  Max: {max:.4e}")
            }
            fem_core::ResultField::NodeVector {
                name,
                min_mag,
                max_mag,
                ..
            } => {
                format!("Result: {name} (magnitude)\nMin: {min_mag:.4e}  Max: {max_mag:.4e}")
            }
            fem_core::ResultField::ElementScalar { name, min, max, .. } => {
                format!("Result: {name}\nMin: {min:.4e}  Max: {max:.4e}")
            }
        }
    } else {
        let total_steps: usize = results.by_mesh.iter().map(|s| s.len()).sum();
        format!("Result: {total_steps} step(s) loaded")
    };
}

/// Steps the result-step slider with the Left/Right arrow keys, so the user
/// doesn't have to drag a thin slider to move one step at a time on a model
/// with many output steps.
///
/// Writes `SliderState::value` directly; the visual sync half of
/// [`crate::slider::update_sliders`] (which must run after this system)
/// picks up the change via `is_changed()` and updates the thumb/fill/text,
/// and [`apply_slider_to_results`] (which must run after that) reads the
/// new value into [`FemResultSet::active`].
// ── animation playback ────────────────────────────────────────────────────────

pub(crate) fn playback_button_system(
    mut playback: ResMut<PlaybackState>,
    results: Option<Res<FemResultSet>>,
    mut play_btns: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &Children,
        ),
        (
            With<PlaybackPlayPauseButton>,
            Without<PlaybackRewindButton>,
            Without<PlaybackEndButton>,
        ),
    >,
    mut rewind_btns: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        (
            With<PlaybackRewindButton>,
            Without<PlaybackPlayPauseButton>,
            Without<PlaybackEndButton>,
        ),
    >,
    mut end_btns: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        (
            With<PlaybackEndButton>,
            Without<PlaybackPlayPauseButton>,
            Without<PlaybackRewindButton>,
        ),
    >,
    mut labels: Query<&mut Text, With<PlaybackPlayPauseLabel>>,
    mut sliders: Query<&mut SliderState, With<SliderTrack>>,
) {
    let step_count = results
        .as_deref()
        .map(|r| r.by_mesh.iter().map(|s| s.len()).max().unwrap_or(0))
        .unwrap_or(0);

    for (interaction, mut bg, mut border, children) in &mut play_btns {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            playback.playing = !playback.playing;
            playback.elapsed = 0.0;
        }
        let active = playback.playing;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);

        for &child in children {
            if let Ok(mut t) = labels.get_mut(child) {
                **t = if playback.playing {
                    "Pause".to_string()
                } else {
                    "Play".to_string()
                };
            }
        }
    }

    for (interaction, mut bg, mut border) in &mut rewind_btns {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            playback.playing = false;
            for mut s in &mut sliders {
                if s.id == SliderId::ResultStep {
                    s.value = 0.0;
                    s.clamp_value();
                }
            }
        }
        *bg = BackgroundColor(if *interaction != Interaction::None {
            BUTTON_HOVERED
        } else {
            BUTTON_NORMAL
        });
        *border = BorderColor::all(PANEL_BORDER);
    }

    for (interaction, mut bg, mut border) in &mut end_btns {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            playback.playing = false;
            let last = (step_count.saturating_sub(1)) as f32;
            for mut s in &mut sliders {
                if s.id == SliderId::ResultStep {
                    s.value = last;
                    s.clamp_value();
                }
            }
        }
        *bg = BackgroundColor(if *interaction != Interaction::None {
            BUTTON_HOVERED
        } else {
            BUTTON_NORMAL
        });
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Advances the result step automatically when [`PlaybackState::playing`]
/// is true, using [`PlaybackState::interval`] as the seconds-per-step.
/// Wraps back to step 0 when the last step is reached (loop mode).
pub(crate) fn playback_advance_system(
    time: Res<Time>,
    mut playback: ResMut<PlaybackState>,
    results: Option<Res<FemResultSet>>,
    mut sliders: Query<&mut SliderState, With<SliderTrack>>,
) {
    if !playback.playing {
        return;
    }

    let step_count = results
        .as_deref()
        .map(|r| r.by_mesh.iter().map(|s| s.len()).max().unwrap_or(0))
        .unwrap_or(0);
    if step_count == 0 {
        playback.playing = false;
        return;
    }

    // Read speed from slider
    let speed = sliders
        .iter()
        .find(|s| s.id == SliderId::PlaybackSpeed)
        .map(|s| s.value)
        .unwrap_or(2.0);
    playback.interval = 1.0 / speed.max(0.1);

    playback.elapsed += time.delta_secs();
    if playback.elapsed < playback.interval {
        return;
    }
    playback.elapsed = 0.0;

    for mut s in &mut sliders {
        if s.id != SliderId::ResultStep {
            continue;
        }
        let next = (s.value + 1.0) % step_count as f32;
        s.value = next;
        s.clamp_value();
    }
}

// ── undo / redo ───────────────────────────────────────────────────────────────

/// Watches for Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z and applies undo/redo.
///
/// The undo stack is populated by any system that modifies
/// [`fem_core::AnalysisSetup`]; those systems call
/// [`UndoStack::push(setup.clone())`] *before* making their change.
pub(crate) fn undo_redo_system(
    keys: Res<ButtonInput<KeyCode>>,
    keyboard_state: Res<fem_core::UiKeyboardState>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut stack: ResMut<UndoStack>,
    mut in_progress: ResMut<UndoInProgress>,
) {
    if keyboard_state.text_editing {
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);

    if ctrl && keys.just_pressed(KeyCode::KeyZ) && !shift {
        if let Some(prev) = stack.undo.pop() {
            let current = std::mem::replace(&mut *setup, prev);
            stack.redo.push(current);
            in_progress.0 = true;
        }
    }

    if ctrl && (keys.just_pressed(KeyCode::KeyY) || (shift && keys.just_pressed(KeyCode::KeyZ))) {
        if let Some(next) = stack.redo.pop() {
            let current = std::mem::replace(&mut *setup, next);
            stack.undo.push(current);
            in_progress.0 = true;
        }
    }
}

/// Watches [`fem_core::AnalysisSetup`] for changes and pushes the *previous*
/// state onto the undo stack **before** each user action commits.
///
/// Uses a `Local` snapshot to capture the pre-change state correctly —
/// `is_changed()` alone only fires *after* the change has already occurred.
/// When an undo/redo itself caused the change (`UndoInProgress`), no
/// snapshot is pushed and the flag is cleared.
pub(crate) fn push_undo_before_setup_change(
    setup: Res<fem_core::AnalysisSetup>,
    mut stack: ResMut<UndoStack>,
    mut in_progress: ResMut<UndoInProgress>,
    mut prev: Local<Option<fem_core::AnalysisSetup>>,
) {
    if setup.is_changed() {
        if in_progress.0 {
            in_progress.0 = false;
        } else if let Some(snapshot) = prev.as_ref() {
            stack.push(snapshot.clone());
        }
        *prev = Some((*setup).clone());
    } else if prev.is_none() {
        *prev = Some((*setup).clone());
    }
}

pub(crate) fn step_keyboard_navigation(
    keyboard: Res<ButtonInput<KeyCode>>,
    keyboard_state: Res<fem_core::UiKeyboardState>,
    results: Res<FemResultSet>,
    mut slider_query: Query<&mut SliderState, With<SliderTrack>>,
) {
    if keyboard_state.text_editing || !results.has_results() {
        return;
    }

    let delta = if keyboard.just_pressed(KeyCode::ArrowRight) {
        1.0
    } else if keyboard.just_pressed(KeyCode::ArrowLeft) {
        -1.0
    } else {
        return;
    };

    for mut state in &mut slider_query {
        if state.id != SliderId::ResultStep {
            continue;
        }

        let new_value = (state.value + delta).clamp(state.min, state.max);

        if (new_value - state.value).abs() > f32::EPSILON {
            state.value = new_value;
        }
    }
}

/// Reads the step slider and deform-scale slider each frame and, when either
/// has changed, updates [`FemResultSet::active`] and
/// [`VisualizationSettings::contour`] so [`update_contour_surface`] re-renders.
///
/// Also shows/hides the slider section and adjusts the step slider's max
/// to match the number of loaded steps.
pub(crate) fn apply_slider_to_results(
    mut results: ResMut<FemResultSet>,
    mut settings: ResMut<visualization::VisualizationSettings>,
    mut section_query: Query<&mut Visibility, With<ResultSliderSection>>,
    mut slider_query: Query<&mut SliderState, With<SliderTrack>>,
) {
    if !results.has_results() {
        if let Ok(mut vis) = section_query.single_mut() {
            *vis = Visibility::Hidden;
        }
        return;
    }

    // Show sliders when results are present.
    if let Ok(mut vis) = section_query.single_mut() {
        *vis = Visibility::Visible;
    }

    let mesh_index = results.active.as_ref().map(|a| a.mesh_index).unwrap_or(0);
    let step_count = results.by_mesh.get(mesh_index).map_or(0, |s| s.len());

    // Read slider values.
    let mut step_value: Option<f32> = None;
    let mut scale_value: Option<f32> = None;

    for mut state in &mut slider_query {
        match state.id {
            SliderId::ResultStep => {
                // Keep max in sync with step count.
                let new_max = (step_count.saturating_sub(1)) as f32;
                if (state.max - new_max).abs() > 0.5 {
                    state.max = new_max;
                    state.clamp_value();
                }
                step_value = Some(state.value);
            }
            SliderId::DeformScale => {
                scale_value = Some(state.value);
            }
            // These sliders are read by dedicated systems; result display doesn't need them.
            SliderId::LoadMagnitude
            | SliderId::SectionThickness
            | SliderId::SurfaceAngle
            | SliderId::DloadMagnitude
            | SliderId::PlaybackSpeed
            | SliderId::AssemblyMovePercent
            | SliderId::AssemblyRotationDegrees
            | SliderId::ContactFriction
            | SliderId::ContactPenaltyFactor
            | SliderId::ContactReviewSeparation => {}
        }
    }

    let step_index = step_value.map(|v| v.round() as usize).unwrap_or(0);

    // Update active step.
    if let Some(active) = results.active.as_mut() {
        if active.step_index != step_index {
            active.step_index = step_index;
            // Signal changed so update_contour_surface re-renders.
            results.set_changed();
        }
    }

    // Update deformation scale in contour settings.
    if let Some(scale) = scale_value {
        if let Some(contour) = settings.contour.as_mut() {
            if (contour.deformation_scale - scale).abs() > 1.0e-4 {
                contour.deformation_scale = scale;
                contour.step_index = step_index;
            }
        }
    }
}

fn selection_level_label(level: SelectionLevel) -> &'static str {
    match level {
        SelectionLevel::Node => "Node",
        SelectionLevel::Edge => "Edge",
        SelectionLevel::Face => "Face",
        SelectionLevel::Element => "Element",
    }
}

/// Formats a mesh-scoped [`FemEntityRef`] for status-line display.
///
/// For elements, appends the FEM element type (e.g. `"Element 354 (Hex8)"`)
/// when `model` is available — this makes mixed element-type meshes
/// self-diagnosing: if an element's rendered shape looks inconsistent with
/// its neighbours (e.g. a solid cuboid surrounded by thin shell plates),
/// hovering it immediately shows whether that's actually a different
/// element type rather than a rendering bug.
fn entity_label(target: FemEntityRef, model: Option<&FemModel>) -> String {
    let entity = match target.entity {
        FemEntityId::Node(id) => format!("Node {}", id.0),
        FemEntityId::Edge(id) => format!("Edge {}", id.0),
        FemEntityId::Face(id) => format!("Face {}", id.0),
        FemEntityId::Element(id) => {
            let type_label = model
                .and_then(|model| model.meshes.get(target.mesh_index))
                .and_then(|mesh| mesh.elements.iter().find(|element| element.id == id))
                .map(|element| element_type_label(&element.element_type));

            match type_label {
                Some(label) => format!("Element {} ({label})", id.0),
                None => format!("Element {}", id.0),
            }
        }
    };

    if model.is_some_and(|model| model.meshes.len() > 1) {
        format!("{} / {entity}", mesh_label(model, target.mesh_index))
    } else {
        entity
    }
}

/// Short display name for a [`fem_core::ElementType`], used in status text.
fn element_type_label(element_type: &fem_core::ElementType) -> String {
    match element_type {
        fem_core::ElementType::Rod2 => "Line2/111".to_string(),
        fem_core::ElementType::Rod3 => "Line3/112".to_string(),
        fem_core::ElementType::Tri3 => "Tri3/231".to_string(),
        fem_core::ElementType::Tri6 => "Tri6/232".to_string(),
        fem_core::ElementType::Quad4 => "Quad4/241".to_string(),
        fem_core::ElementType::Quad8 => "Quad8/242".to_string(),
        fem_core::ElementType::Truss2 => "Truss2/301".to_string(),
        fem_core::ElementType::Tet4 => "Tet4/341".to_string(),
        fem_core::ElementType::Tet10 => "Tet10/342".to_string(),
        fem_core::ElementType::Prism6 => "Prism6/351".to_string(),
        fem_core::ElementType::Prism15 => "Prism15/352".to_string(),
        fem_core::ElementType::Hex8 => "Hex8/361".to_string(),
        fem_core::ElementType::Hex20 => "Hex20/362".to_string(),
        fem_core::ElementType::Connector2 => "Connector2/511".to_string(),
        fem_core::ElementType::InterfaceQuad4 => "InterfaceQuad4/541".to_string(),
        fem_core::ElementType::InterfaceQuad8 => "InterfaceQuad8/542".to_string(),
        fem_core::ElementType::Beam611 => "Beam2/611".to_string(),
        fem_core::ElementType::Beam641 => "MixedBeam2/641".to_string(),
        fem_core::ElementType::ShellTri3 => "ShellTri3/731".to_string(),
        fem_core::ElementType::ShellTri6 => "ShellTri6/732".to_string(),
        fem_core::ElementType::ShellQuad4 => "ShellQuad4/741".to_string(),
        fem_core::ElementType::ShellQuad9 => "ShellQuad9/743".to_string(),
        fem_core::ElementType::ShellTri3Mixed => "MixedShellTri3/761".to_string(),
        fem_core::ElementType::ShellQuad4Mixed => "MixedShellQuad4/781".to_string(),
        fem_core::ElementType::Unsupported(name) => format!("?{name}"),
    }
}

fn load_status_line(status: &MeshLoadStatus) -> String {
    let path = status
        .last_path
        .as_deref()
        .map(compact_path)
        .unwrap_or_else(|| "No file".to_string());

    if let Some(error) = &status.error {
        format!("File: {path}  Error: {error}")
    } else if status.message.is_empty() {
        format!("File: {path}")
    } else {
        format!("File: {path}  {}", status.message)
    }
}

fn compact_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod sidebar_page_tests {
    use std::path::PathBuf;

    use super::{
        CameraFitRequest, ContactDefinitionSettings, ContactPairKind, SELECTION_GUIDE_TEXT,
        SelectionGuideState, SidebarPage, SidebarPageContent, SurfaceSelectionMode,
        SurfaceSelectionSettings, apply_mesh, create_contact_from_draft, merge_mesh_contact_pairs,
        page_supports_part_position, selected_nodes_by_mesh, selection_context_for_page,
        selection_operation_hint, sidebar_page_display, signed_preview_direction,
        supports_surface_growth, surface_selection_hint, sync_contact_measurement_box,
        update_hover_preview_group,
    };
    use crate::measurement::{MeasurementBoxState, MeasurementTarget};
    use bevy::prelude::{App, Display, Update, Vec3};
    use fem_core::{
        AnalysisSetup, ElementId, ElementType, FemElement, FemEntityId, FemEntityRef, FemMesh,
        FemModel, FemModelVersion, FemNode, HoverPreviewTargets, MeshLoadStatus, NodeId,
        SelectionHit, SelectionLevel, ViewportTool,
    };
    use interaction::HoverResult;
    use selection::{SelectionOperation, SelectionState};
    use visualization::{ContactDraftPreview, ContactDraftSlave, ContactDraftSurface};

    #[test]
    fn analysis_shell_is_limited_to_analysis_pages() {
        let pages = SidebarPageContent::analysis();

        assert!(!pages.contains(SidebarPage::Model));
        assert!(!pages.contains(SidebarPage::Contact));
        assert!(pages.contains(SidebarPage::Loads));
        assert!(pages.contains(SidebarPage::Materials));
        assert!(pages.contains(SidebarPage::Solve));
        assert!(!pages.contains(SidebarPage::Results));
    }

    #[test]
    fn part_position_controls_are_shared_by_model_and_contact() {
        let pages = SidebarPageContent::part_position();

        assert!(pages.contains(SidebarPage::Model));
        assert!(pages.contains(SidebarPage::Contact));
        assert!(!pages.contains(SidebarPage::Loads));
        assert!(page_supports_part_position(SidebarPage::Model));
        assert!(page_supports_part_position(SidebarPage::Contact));
        assert!(!page_supports_part_position(SidebarPage::Materials));
    }

    #[test]
    fn contact_measurement_sync_preserves_part_position_input() {
        let mut measurement = MeasurementBoxState::default();
        measurement.begin_assembly_translation(0, Vec3::X);

        let mut app = App::new();
        app.insert_resource(SidebarPage::Contact);
        app.insert_resource(ViewportTool::Assembly);
        app.init_resource::<ContactDefinitionSettings>();
        app.insert_resource(measurement);
        app.add_systems(Update, sync_contact_measurement_box);
        app.update();

        assert!(matches!(
            app.world().resource::<MeasurementBoxState>().target,
            Some(MeasurementTarget::AssemblyTranslation { .. })
        ));
    }

    #[test]
    fn single_page_masks_do_not_leak_to_other_pages() {
        let all_pages = [
            SidebarPage::Model,
            SidebarPage::Contact,
            SidebarPage::Loads,
            SidebarPage::Materials,
            SidebarPage::Solve,
            SidebarPage::Results,
        ];

        for selected in all_pages {
            let content = SidebarPageContent::page(selected);
            for candidate in all_pages {
                assert_eq!(content.contains(candidate), selected == candidate);
            }
        }
    }

    #[test]
    fn inactive_sidebar_pages_are_removed_from_layout() {
        let contact = SidebarPageContent::page(SidebarPage::Contact);

        assert_eq!(
            sidebar_page_display(contact, SidebarPage::Contact),
            Display::Flex
        );
        assert_eq!(
            sidebar_page_display(contact, SidebarPage::Model),
            Display::None
        );
    }

    #[test]
    fn selection_targets_follow_the_active_workflow() {
        let model = selection_context_for_page(SidebarPage::Model);
        assert_eq!(model.levels.len(), 4);

        let contact = selection_context_for_page(SidebarPage::Contact);
        assert_eq!(
            contact.levels,
            &[
                SelectionLevel::Node,
                SelectionLevel::Face,
                SelectionLevel::Element,
            ]
        );
        assert_eq!(contact.preferred, SelectionLevel::Node);

        let loads = selection_context_for_page(SidebarPage::Loads);
        assert!(loads.levels.contains(&SelectionLevel::Node));
        assert!(!loads.levels.contains(&SelectionLevel::Edge));

        let materials = selection_context_for_page(SidebarPage::Materials);
        assert_eq!(materials.levels, &[SelectionLevel::Element]);

        assert!(
            selection_context_for_page(SidebarPage::Solve)
                .levels
                .is_empty()
        );
        assert!(
            selection_context_for_page(SidebarPage::Results)
                .levels
                .is_empty()
        );
    }

    #[test]
    fn node_surface_draft_creates_groups_and_contact_only_when_finalized() {
        let mut model = FemModel::demo_hex8();
        let master_face = model.meshes[0].cached_boundary_faces()[0]
            .element_face_ref()
            .unwrap();
        let draft = ContactDraftPreview {
            master: Some(ContactDraftSurface {
                mesh_index: 0,
                surfaces: vec![master_face],
            }),
            slave: Some(ContactDraftSlave::Nodes {
                mesh_index: 0,
                nodes: vec![NodeId(0), NodeId(1)],
            }),
            active: true,
        };

        assert!(model.contacts.is_empty());
        assert!(model.meshes[0].node_sets.is_empty());
        assert!(model.meshes[0].surface_sets.is_empty());

        let index = create_contact_from_draft(
            &mut model,
            &draft,
            ContactPairKind::NodeSurface,
            fem_core::ContactType::FiniteSliding,
            0.15,
            Some(2.5e5),
        )
        .unwrap();

        assert_eq!(index, 0);
        assert_eq!(model.meshes[0].node_sets[0].nodes.len(), 2);
        assert_eq!(model.meshes[0].surface_sets[0].surfaces.len(), 1);
        assert_eq!(
            model.contacts[0].slave,
            fem_core::ContactSlaveRef::Nodes(fem_core::NodeSetRef::new(0, 0))
        );
        assert_eq!(
            model.contacts[0].contact_type,
            fem_core::ContactType::FiniteSliding
        );
        assert_eq!(model.contacts[0].friction_coefficient, 0.15);
        assert_eq!(model.contacts[0].penalty_factor, Some(2.5e5));
    }

    #[test]
    fn sliding_contact_rejects_invalid_parameters_before_creating_groups() {
        let mut model = FemModel::demo_hex8();
        let master_face = model.meshes[0].cached_boundary_faces()[0]
            .element_face_ref()
            .unwrap();
        let draft = ContactDraftPreview {
            master: Some(ContactDraftSurface {
                mesh_index: 0,
                surfaces: vec![master_face],
            }),
            slave: Some(ContactDraftSlave::Nodes {
                mesh_index: 0,
                nodes: vec![NodeId(0)],
            }),
            active: true,
        };

        let friction_error = create_contact_from_draft(
            &mut model,
            &draft,
            ContactPairKind::NodeSurface,
            fem_core::ContactType::FiniteSliding,
            -0.1,
            None,
        )
        .unwrap_err();
        assert!(friction_error.contains("Friction coefficient"));

        let penalty_error = create_contact_from_draft(
            &mut model,
            &draft,
            ContactPairKind::NodeSurface,
            fem_core::ContactType::FiniteSliding,
            0.1,
            Some(0.0),
        )
        .unwrap_err();
        assert!(penalty_error.contains("penalty factor"));
        assert!(model.contacts.is_empty());
        assert!(model.meshes[0].node_sets.is_empty());
        assert!(model.meshes[0].surface_sets.is_empty());
    }

    #[test]
    fn replacing_a_mesh_clears_setup_inside_the_load_transaction() {
        let mut model = FemModel::demo_hex8();
        let mut setup = AnalysisSetup::default();
        setup.add_constraint(0, vec![NodeId(0)], 1, 3, 0.0);
        let mut status = MeshLoadStatus::default();
        let mut version = FemModelVersion::default();
        let mut camera_fit = CameraFitRequest::default();

        apply_mesh(
            FemMesh::demo_hex8(),
            &PathBuf::from("replacement.msh"),
            false,
            &mut model,
            &mut status,
            &mut version,
            &mut camera_fit,
            &mut setup,
        );

        assert!(setup.is_empty());
        assert_eq!(version.value, 1);
        assert_eq!(camera_fit.revision, 1);
    }

    #[test]
    fn importing_an_assembly_part_preserves_existing_setup() {
        let mut model = FemModel::demo_hex8();
        let mut setup = AnalysisSetup::default();
        setup.add_constraint(0, vec![NodeId(0)], 1, 3, 0.0);
        let mut status = MeshLoadStatus::default();
        let mut version = FemModelVersion::default();
        let mut camera_fit = CameraFitRequest::default();

        apply_mesh(
            FemMesh::demo_hex8(),
            &PathBuf::from("part.msh"),
            true,
            &mut model,
            &mut status,
            &mut version,
            &mut camera_fit,
            &mut setup,
        );

        assert_eq!(setup.boundary_conditions.len(), 1);
        assert_eq!(model.meshes.len(), 2);
    }

    #[test]
    fn mesh_contact_pair_resolves_tutorial_node_surface_groups() {
        let mut model = FemModel::demo_hex8();
        model.meshes[0].node_sets.push(fem_core::FemNodeSet {
            name: "slave".to_string(),
            nodes: vec![NodeId(0)],
        });
        model.meshes[0]
            .surface_sets
            .push(fem_core::FemSurfaceSet::new("master"));

        let count = merge_mesh_contact_pairs(
            &mut model,
            0,
            vec![hecmw::HecmwContactPairDefinition {
                name: "CP1".to_string(),
                pair_type: hecmw::HecmwContactPairType::NodeSurface,
                slave_group_name: "SLAVE".to_string(),
                master_surface_name: "MASTER".to_string(),
            }],
        );

        assert_eq!(count, 1);
        assert_eq!(model.contacts[0].name, "CP1");
        assert_eq!(model.contacts[0].master, fem_core::SurfaceSetRef::new(0, 0));
        assert_eq!(
            model.contacts[0].slave,
            fem_core::ContactSlaveRef::Nodes(fem_core::NodeSetRef::new(0, 0))
        );
        assert_eq!(
            model.contacts[0].contact_type,
            fem_core::ContactType::SmallSliding
        );
    }

    #[test]
    fn selected_nodes_remain_partitioned_by_mesh() {
        let selection = SelectionState {
            targets: vec![
                FemEntityRef::node(0, NodeId(7)),
                FemEntityRef::node(1, NodeId(7)),
            ],
            ..Default::default()
        };

        let grouped = selected_nodes_by_mesh(&selection);

        assert_eq!(grouped.get(&0), Some(&vec![NodeId(7)]));
        assert_eq!(grouped.get(&1), Some(&vec![NodeId(7)]));
    }

    #[test]
    fn load_preview_direction_tracks_the_sign_but_hides_zero_loads() {
        assert_eq!(signed_preview_direction(Vec3::X, 12.0), Vec3::X);
        assert_eq!(signed_preview_direction(Vec3::X, -12.0), Vec3::NEG_X);
        assert_eq!(signed_preview_direction(Vec3::X, 0.0), Vec3::ZERO);
    }

    #[test]
    fn surface_growth_hint_keeps_face_and_element_meanings_distinct() {
        assert_eq!(
            surface_selection_hint(SelectionLevel::Face, SurfaceSelectionMode::Smooth),
            "Face Smooth = connected curved surface patch"
        );
        assert_eq!(
            surface_selection_hint(SelectionLevel::Element, SurfaceSelectionMode::Smooth),
            "Element Smooth = whole elements behind curved patch"
        );
    }

    #[test]
    fn surface_growth_controls_only_apply_to_face_and_element() {
        assert!(!supports_surface_growth(SelectionLevel::Node));
        assert!(!supports_surface_growth(SelectionLevel::Edge));
        assert!(supports_surface_growth(SelectionLevel::Face));
        assert!(supports_surface_growth(SelectionLevel::Element));
    }

    #[test]
    fn selection_guide_starts_open_and_names_every_modifier_operation() {
        assert!(SelectionGuideState::default().expanded);
        assert!(SELECTION_GUIDE_TEXT.contains("Double click"));
        assert!(SELECTION_GUIDE_TEXT.contains("Triple click"));
        assert!(
            selection_operation_hint(SelectionOperation::Replace)
                .0
                .contains("REPLACE")
        );
        assert!(
            selection_operation_hint(SelectionOperation::Add)
                .0
                .contains("ADD")
        );
        assert!(
            selection_operation_hint(SelectionOperation::Toggle)
                .0
                .contains("TOGGLE")
        );
        assert!(
            selection_operation_hint(SelectionOperation::Remove)
                .0
                .contains("REMOVE")
        );
    }

    #[test]
    fn element_surface_growth_keeps_element_targets_but_highlights_faces() {
        let model = FemModel::demo_hex8();
        let face = model.meshes[0].cached_boundary_faces()[0].clone();
        let element = face.element.expect("a solid boundary face has an owner");
        let hit = SelectionHit::new(FemEntityRef::element(0, element), Vec3::ZERO, 0.0)
            .with_surface(face.id, face.element);

        let mut app = App::new();
        app.insert_resource(model);
        app.insert_resource(HoverResult {
            entity: None,
            hit: Some(hit),
        });
        app.insert_resource(SurfaceSelectionSettings {
            mode: SurfaceSelectionMode::Coplanar,
        });
        app.init_resource::<HoverPreviewTargets>();
        app.add_systems(Update, update_hover_preview_group);

        app.update();

        let preview = app.world().resource::<HoverPreviewTargets>();
        assert!(
            preview
                .targets
                .iter()
                .all(|target| matches!(target.entity, FemEntityId::Element(_)))
        );
        assert!(
            preview
                .highlight_targets
                .iter()
                .all(|target| matches!(target.entity, FemEntityId::Face(_)))
        );
        assert!(!preview.targets.is_empty());
        assert!(!preview.highlight_targets.is_empty());
    }

    #[test]
    fn edge_hover_previews_a_continuous_feature_chain() {
        let mesh = FemMesh::new(
            vec![
                FemNode::new(NodeId(0), Vec3::new(0.0, 0.0, 0.0)),
                FemNode::new(NodeId(1), Vec3::new(1.0, 0.0, 0.0)),
                FemNode::new(NodeId(2), Vec3::new(1.0, 1.0, 0.0)),
                FemNode::new(NodeId(3), Vec3::new(0.0, 1.0, 0.0)),
                FemNode::new(NodeId(4), Vec3::new(2.0, 0.0, 0.0)),
                FemNode::new(NodeId(5), Vec3::new(2.0, 1.0, 0.0)),
            ],
            vec![
                FemElement::new(
                    ElementId(0),
                    ElementType::ShellQuad4,
                    vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
                ),
                FemElement::new(
                    ElementId(1),
                    ElementType::ShellQuad4,
                    vec![NodeId(1), NodeId(4), NodeId(5), NodeId(2)],
                ),
            ],
        );
        let seed = mesh
            .cached_boundary_edges()
            .iter()
            .find(|edge| edge.nodes.contains(&NodeId(0)) && edge.nodes.contains(&NodeId(1)))
            .expect("bottom-left edge")
            .id;
        let model = FemModel::single_mesh("shells", mesh);
        let hit = SelectionHit::new(FemEntityRef::edge(0, seed), Vec3::ZERO, 0.0);

        let mut app = App::new();
        app.insert_resource(model);
        app.insert_resource(HoverResult {
            entity: None,
            hit: Some(hit),
        });
        app.insert_resource(SurfaceSelectionSettings {
            mode: SurfaceSelectionMode::Smooth,
        });
        app.init_resource::<HoverPreviewTargets>();
        app.add_systems(Update, update_hover_preview_group);

        app.update();

        let preview = app.world().resource::<HoverPreviewTargets>();
        assert_eq!(preview.targets.len(), 2);
        assert_eq!(preview.highlight_targets, preview.targets);
        assert!(
            preview
                .targets
                .iter()
                .all(|target| matches!(target.entity, FemEntityId::Edge(_)))
        );
    }
}
