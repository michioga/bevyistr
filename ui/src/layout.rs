use bevy::prelude::*;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::ui::ScrollPosition;
use camera::OrbitCamera;
use fem_core::{
    ContactCandidateState, ContactType, FemEntityId, FemModel, FemModelVersion, FemResultSet,
    MeshLoadRequest, MeshLoadStatus, SelectionFilter, SelectionLevel, UiPointerState,
};
use crate::slider::{spawn_slider, SliderConfig, SliderId, SliderState, SliderTrack};
use visualization::ContourSettings;
use interaction::HoverResult;
use selection::{Hovered, Selectable, Selected, SelectionState};
use std::path::Path;
use visualization::{VisualizationMode, VisualizationSettings};

const PANEL_BG: Color = Color::srgba(0.035, 0.04, 0.045, 0.88);
const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);
const ACTIVE_BORDER: Color = Color::srgb(0.57, 0.86, 0.92);

#[derive(Component)]
pub(crate) struct SelectionLevelButton {
    level: SelectionLevel,
}

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

/// Resource controlling the coplanar-face-expansion feature.
///
/// When `enabled`, clicking a face or element in the 3D view expands the
/// selection to all connected boundary faces whose normal deviates by at
/// most `angle_deg` from the clicked face's normal.
///
/// The angle can be changed with a slider while hovering; the live
/// coplanar-group *preview* (see [`fem_core::HoverPreviewTargets`], built
/// each frame by [`update_hover_preview_group`]) re-runs the walk from
/// whatever's under the cursor every frame, so the preview always reflects
/// the current threshold before you ever click — essential for "feel" on
/// curved or chamfered geometry where the right threshold isn't obvious in
/// advance. What actually gets added to the selection on a click is
/// whatever the preview showed at that moment (see
/// [`selection::click_selection_system`]), so this resource only needs to
/// track the toggle and the threshold — no click-time seed bookkeeping.
#[derive(Resource, Debug, Clone)]
pub(crate) struct PlanarSelectionSettings {
    pub enabled:   bool,
    pub angle_deg: f32,
}

impl Default for PlanarSelectionSettings {
    fn default() -> Self {
        Self {
            enabled:      false,
            angle_deg:    15.0,
        }
    }
}

#[derive(Component)]
pub(crate) struct PlanarSelectionToggle;

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

#[derive(Component)]
pub(crate) struct DetectContactsButton;

#[derive(Component)]
pub(crate) struct AcceptContactButton;

#[derive(Component)]
pub(crate) struct MeshStatsText;

#[derive(Component)]
pub(crate) struct SelectionStatsText;

#[derive(Component)]
pub(crate) struct ContactCandidateText;

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
        Self { playing: false, interval: 0.2, elapsed: 0.0 }
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
pub(crate) struct PartsListText;

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

            sidebar_page_group(panel, SidebarPageContent::page(SidebarPage::Model), "ModelPage", |panel| {

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
                    Text::new(""),
                    TextFont { font_size: FontSize::Px(11.0), ..default() },
                    TextColor(TEXT_MUTED),
                    PartsListText,
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

            // ── § View ──────────────────────────────────────────────────
            section(panel, "VIEW", |sec| {
                sec.spawn((
                    Node { flex_direction: FlexDirection::Row, ..default() },
                )).with_children(|row| {
                    for (i, mode) in VisualizationMode::ALL.iter().enumerate() {
                        let n = VisualizationMode::ALL.len();
                        let (radius, border) = segment_style(i == 0, i == n - 1);
                        row.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: px(28.0),
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
                        )).with_child((
                            Text::new(mode.label()),
                            TextFont { font_size: FontSize::Px(11.5), ..default() },
                            TextColor(TEXT_MAIN),
                        ));
                    }
                });
                hint_text(sec, "Middle drag = orbit   Shift+Middle = pan   Scroll = zoom   F = focus");
            });
            divider(panel);

            // ── § Selection ─────────────────────────────────────────────
            section(panel, "SELECTION", |sec| {
                sec.spawn((
                    Text::new("Filter: Element   Selected: 0   Hover: none"),
                    TextFont { font_size: FontSize::Px(11.5), ..default() },
                    TextColor(TEXT_MAIN),
                    SelectionStatsText,
                ));
                hint_text(sec, "Click = select   Ctrl/Shift+Click = add   Alt+Click = remove   Drag = box select");
                // Dynamic info: count + hover coords — updated every frame.
                sec.spawn((
                    Text::new("Selected: 0  |  Hover: -"),
                    TextFont { font_size: FontSize::Px(11.0), ..default() },
                    TextColor(Color::srgba(0.50, 0.78, 0.95, 0.90)),
                    SelectionInfoText,
                ));

                // ── Planar selection ──────────────────────────────────────
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(8.0),
                        margin: UiRect::top(px(4.0)),
                        ..default()
                    },
                )).with_children(|row| {
                    row.spawn((
                        Button,
                        Node {
                            padding: UiRect::axes(px(10.0), px(4.0)),
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(5.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(BUTTON_NORMAL),
                        BorderColor::all(PANEL_BORDER),
                        PlanarSelectionToggle,
                        Name::new("PlanarToggle"),
                    )).with_child((
                        Text::new("Planar OFF"),
                        TextFont { font_size: FontSize::Px(11.0), ..default() },
                        TextColor(TEXT_MAIN),
                    ));
                });
                spawn_slider(sec, SliderConfig {
                    width: 272.0,
                    min: 0.0, max: 90.0, value: 15.0,
                    label: "Angle threshold (deg)",
                    id: SliderId::PlanarAngle,
                });
                hint_text(sec, "Click face/element to select coplanar neighbours");

                sec.spawn((
                    Node { flex_direction: FlexDirection::Row, column_gap: px(6.0), ..default() },
                )).with_children(|row| {
                    action_button(row, "Make Node Group",    MakeNodeGroupButton,    "MakeNodeGroupButton");
                    action_button(row, "Make Element Group", MakeElementGroupButton, "MakeElementGroupButton");
                });
                hint_text(sec, "Saves selection as NGRP/EGRP for use in BCs and sections");
            });
            divider(panel);

            }); // end ModelPage

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

            // ── § Contact Setup ──────────────────────────────────────────
            section(panel, "CONTACT SETUP", |sec| {
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(6.0),
                        ..default()
                    },
                )).with_children(|row| {
                    action_button(row, "Make Surface",    CreateSurfaceButton,  "CreateSurfaceButton");
                    action_button(row, "Make Contact",    CreateContactButton,  "CreateContactButton");
                });
                sec.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(6.0),
                        ..default()
                    },
                )).with_children(|row| {
                    action_button(row, "Detect Contacts", DetectContactsButton, "DetectContactsButton");
                    action_button(row, "Accept",          AcceptContactButton,  "AcceptContactButton");
                });
                sec.spawn((
                    Text::new("Contacts: no candidates (press Detect)"),
                    TextFont { font_size: FontSize::Px(11.5), ..default() },
                    TextColor(TEXT_MUTED),
                    ContactCandidateText,
                ));
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
                        hint_text(dp, "Use Face filter, select faces, pick kind, Apply");
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
                    hint_text(panel, "Select nodes first - buttons show count");

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
            nav.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4.0),
                    ..default()
                },
            ))
            .with_children(|row| {
                sidebar_page_button(row, SidebarPage::Model, "Model");
                sidebar_page_button(row, SidebarPage::Contact, "Contact");
                sidebar_page_button(row, SidebarPage::Loads, "BC / Loads");
            });
            nav.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4.0),
                    ..default()
                },
            ))
            .with_children(|row| {
                sidebar_page_button(row, SidebarPage::Materials, "Materials");
                sidebar_page_button(row, SidebarPage::Solve, "Solve");
                sidebar_page_button(row, SidebarPage::Results, "Results");
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
                flex_direction: FlexDirection::Row,
                padding: UiRect::axes(px(10.0), px(0.0)),
                ..default()
            },
            Name::new("SelectionLevelBar"),
        ))
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
}

fn sidebar_page_button(
    parent: &mut ChildSpawnerCommands,
    page: SidebarPage,
    label: &'static str,
) {
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
    let initial_visibility = if pages.contains(SidebarPage::Model) {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };

    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
            pages,
            initial_visibility,
            Name::new(name),
        ))
        .with_children(children_fn);
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
                TextFont { font_size: FontSize::Px(9.5), ..default() },
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
    parent.spawn((
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
    )).with_child((
        Text::new(label),
        TextFont { font_size: FontSize::Px(11.5), ..default() },
        TextColor(TEXT_MAIN),
    ));
}

fn constraint_preset_button(parent: &mut ChildSpawnerCommands, label: &'static str, dof_start: u8, dof_end: u8) {
    parent.spawn((
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
        ConstraintPresetButton { dof_start, dof_end, label },
        Name::new(format!("ConstraintPreset_{label}")),
    )).with_child((
        Text::new(label),
        TextFont { font_size: FontSize::Px(9.5), ..default() },
        TextColor(TEXT_MAIN),
        ConstraintPresetLabel,   // ← enables dynamic count label
    ));
}

fn load_direction_button(parent: &mut ChildSpawnerCommands, label: &'static str, dof: u8, sign: f32) {
    parent.spawn((
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
    )).with_child((
        Text::new(label),
        TextFont { font_size: FontSize::Px(9.5), ..default() },
        TextColor(TEXT_MAIN),
    ));
}

fn material_preset_button(parent: &mut ChildSpawnerCommands, preset_index: usize, label: &'static str) {
    parent.spawn((
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
    )).with_child((
        Text::new(label),
        TextFont { font_size: FontSize::Px(9.5), ..default() },
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
        MaterialPreset { label: "+ Steel",    name: "STEEL",    young_modulus: 2.05e11, poisson_ratio: 0.30, density: 7850.0 },
        MaterialPreset { label: "+ Aluminum", name: "ALUMINUM", young_modulus: 6.90e10, poisson_ratio: 0.33, density: 2700.0 },
        MaterialPreset { label: "+ Concrete", name: "CONCRETE", young_modulus: 3.00e10, poisson_ratio: 0.20, density: 2400.0 },
        MaterialPreset { label: "+ Titanium", name: "TITANIUM", young_modulus: 1.14e11, poisson_ratio: 0.34, density: 4500.0 },
    ];

    PRESETS
}

fn hint_text(parent: &mut ChildSpawnerCommands, text: &'static str) {
    parent.spawn((
        Text::new(text),
        TextFont { font_size: FontSize::Px(10.0), ..default() },
        TextColor(Color::srgba(0.45, 0.54, 0.60, 0.80)),
    ));
}

fn segment_style(is_first: bool, is_last: bool) -> (BorderRadius, UiRect) {
    let r = 5.0f32;
    let border = UiRect {
        top:    px(1.0),
        bottom: px(1.0),
        left:   if is_first { px(1.0) } else { px(0.0) },
        right:  px(1.0),
    };
    let (tl, bl) = if is_first { (r, r) } else { (0.0, 0.0) };
    let (tr, br) = if is_last  { (r, r) } else { (0.0, 0.0) };
    (BorderRadius::new(px(tl), px(tr), px(br), px(bl)), border)
}

/// Handles sidebar page selection, paints the active page button, and resets
/// the content scroll position whenever the task changes.
pub(crate) fn sidebar_page_button_system(
    mut page: ResMut<SidebarPage>,
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
            *page = button.page;
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

/// Shows only content associated with the current sidebar task. Nested
/// masks are supported: the analysis shell is visible for Loads, Materials,
/// and Solve while its children select one of those pages.
pub(crate) fn update_sidebar_page_visibility(
    page: Res<SidebarPage>,
    mut content: Query<(&SidebarPageContent, &mut Visibility)>,
) {
    if !page.is_changed() {
        return;
    }

    for (pages, mut visibility) in &mut content {
        *visibility = if pages.contains(*page) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
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
    let Ok(window) = windows.single() else { return; };
    let Some(cursor) = window.cursor_position() else { return; };

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
            let scale  = node.inverse_scale_factor;
            let size   = node.size() * scale;
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
    let Ok(window) = windows.single() else { return; };
    let Some(cursor) = window.cursor_position() else { return; };

    // If cursor is over any ScrollableList, the list handler takes priority.
    let over_sublist = list_query.iter().any(|(node, transform)| {
        let scale  = node.inverse_scale_factor;
        let size   = node.size() * scale;
        let origin = transform.transform_point2(Vec2::ZERO) * scale - size * 0.5;
        cursor.x >= origin.x && cursor.x <= origin.x + size.x
        && cursor.y >= origin.y && cursor.y <= origin.y + size.y
    });

    if over_sublist { return; }

    for ev in wheel_events.read() {
        let line_height = 28.0;
        let delta_y = match ev.unit {
            MouseScrollUnit::Line  => ev.y * line_height,
            MouseScrollUnit::Pixel => ev.y,
        };

        if delta_y == 0.0 { continue; }

        for (mut scroll, node, transform) in &mut panel_query {
            let scale  = node.inverse_scale_factor;
            let size   = node.size() * scale;
            let origin = transform.transform_point2(Vec2::ZERO) * scale - size * 0.5;

            let over_panel = cursor.x >= origin.x && cursor.x <= origin.x + size.x
                && cursor.y >= origin.y && cursor.y <= origin.y + size.y;

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
    for (_entity, interaction, button, mut background, mut border, mut bevy_button) in &mut buttons {
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
    mut model:   ResMut<FemModel>,
    mut request: ResMut<MeshLoadRequest>,
    mut status:  ResMut<MeshLoadStatus>,
    mut version: ResMut<FemModelVersion>,
    mut setup:   ResMut<fem_core::AnalysisSetup>,
) {
    let Some((path, import)) = request.take() else { return; };

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();

    match ext.as_str() {
        "geo" => {
            match gmsh::run_gmsh(&path, None) {
                Ok(mesh) => { apply_mesh(mesh, &path, import, &mut model, &mut status, &mut version, &mut setup); }
                Err(e)   => { status.failed(path, e.to_string()); }
            }
        }
        "inp" => {
            match hecmw::load_inp_file(&path) {
                Ok(mesh) => { apply_mesh(mesh, &path, import, &mut model, &mut status, &mut version, &mut setup); }
                Err(e)   => { status.failed(path, e.to_string()); }
            }
        }
        _ => {
            // .msh: HECMW extended loader captures !MATERIAL/!SECTION, then Gmsh fallback.
            match hecmw::load_mesh_file_with_setup(&path) {
                Ok((mesh, materials, sections)) => {
                    apply_mesh(mesh, &path, import, &mut model, &mut status, &mut version, &mut setup);
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
                    if changed { setup.set_changed(); }
                }
                Err(_) => {
                    match gmsh::load_msh_file(&path) {
                        Ok(mesh) => { apply_mesh(mesh, &path, import, &mut model, &mut status, &mut version, &mut setup); }
                        Err(e)   => { status.failed(path, e.to_string()); }
                    }
                }
            }
        }
    }
}

fn apply_mesh(
    mesh:    fem_core::FemMesh,
    path:    &std::path::PathBuf,
    import:  bool,
    model:   &mut FemModel,
    status:  &mut MeshLoadStatus,
    version: &mut FemModelVersion,
    setup:   &mut fem_core::AnalysisSetup,
) {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("mesh").to_string();
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
}

/// Recenters and re-scales the orbit camera to fit the model's bounds
/// whenever [`FemModelVersion`] changes (e.g. after a mesh file is loaded).
///
/// The first invocation (at startup) is skipped, since the app's startup
/// `setup` system already places the camera for the initial model.
pub(crate) fn camera_refit_on_reload(
    model: Option<Res<FemModel>>,
    version: Res<FemModelVersion>,
    mut last_version: Local<Option<u64>>,
    mut camera_query: Query<(&mut Transform, &mut OrbitCamera)>,
) {
    let current = version.value;

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
                    SetButton { mesh_index, kind: SetKind::Node, set_index },
                    &format!("[N] {}  ({} nodes)", set.name, set.nodes.len()),
                );
            }

            for (set_index, set) in mesh.element_sets.iter().enumerate() {
                set_list_button(
                    list,
                    SetButton { mesh_index, kind: SetKind::Element, set_index },
                    &format!("[E] {}  ({} elems)", set.name, set.elements.len()),
                );
            }

            for (set_index, set) in mesh.surface_sets.iter().enumerate() {
                set_list_button(
                    list,
                    SetButton { mesh_index, kind: SetKind::Surface, set_index },
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

    let Ok(container) = container_query.single() else { return; };

    if let Ok(children) = children_query.get(container) {
        for &child in children { commands.entity(child).despawn(); }
    }

    commands.entity(container).with_children(|list| {
        for (index, bc) in setup.boundary_conditions.iter().enumerate() {
            let label = format!("[BC] {}  {}  ({} nodes)  val={:.4}", bc.name, bc.dof_label(), bc.nodes.len(), bc.value);
            setup_entry_row(list, &label, DeleteSetupEntry::BoundaryCondition(index), &format!("BC_{}", bc.name));
        }

        // Group nodal loads by name for display (one entry per unique name).
        let mut seen_load_names: Vec<&str> = Vec::new();
        for (index, load) in setup.nodal_loads.iter().enumerate() {
            if seen_load_names.contains(&load.name.as_str()) { continue; }
            seen_load_names.push(&load.name);
            let dof_label = match load.dof { 1 => "Fx", 2 => "Fy", 3 => "Fz", _ => "?" };
            let count = setup.nodal_loads.iter().filter(|l| l.name == load.name).count();
            let label = format!("[Load] {}  {}={:.3}  ({} nodes)", load.name, dof_label, load.value, count);
            setup_entry_row(list, &label, DeleteSetupEntry::LoadGroup(index), &format!("Load_{}", load.name));
        }

        for (index, dload) in setup.distributed_loads.iter().enumerate() {
            let kind_label = match dload.kind {
                fem_core::DistributedLoadKind::Pressure => "Pressure",
                fem_core::DistributedLoadKind::Gravity  => "Gravity",
            };
            let unit = match dload.kind {
                fem_core::DistributedLoadKind::Pressure => "faces",
                fem_core::DistributedLoadKind::Gravity  => "elems",
            };
            let label = format!("[DLoad] {}  {kind_label}={:.3}  ({} {unit})", dload.name, dload.value, dload.target.len());
            setup_entry_row(list, &label, DeleteSetupEntry::DistributedLoad(index), &format!("DLoad_{}", dload.name));
        }

        if setup.boundary_conditions.is_empty() && setup.nodal_loads.is_empty() && setup.distributed_loads.is_empty() {
            list.spawn((
                Text::new("(none yet - select nodes and use buttons above)"),
                TextFont { font_size: FontSize::Px(9.5), ..default() },
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
// ── planar selection ──────────────────────────────────────────────────────────

/// Toggles the `PlanarSelectionSettings::enabled` flag and updates the
/// button label to "Planar ON" / "Planar OFF".
pub(crate) fn planar_selection_toggle_system(
    mut planar:  ResMut<PlanarSelectionSettings>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor, &Children),
        With<PlanarSelectionToggle>,
    >,
    mut labels:  Query<&mut Text>,
) {
    for (interaction, mut bg, mut border, children) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            planar.enabled = !planar.enabled;
        }

        let active = planar.enabled;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);

        for &child in children {
            if let Ok(mut text) = labels.get_mut(child) {
                **text = if planar.enabled { "Planar  ON".to_string() } else { "Planar OFF".to_string() };
            }
        }
    }
}

/// Computes the live "what would clicking select right now" preview group
/// (see [`fem_core::HoverPreviewTargets`]) from the current hover target
/// and the planar/coplanar settings, every frame.
///
/// This always walks from whatever is under the cursor *this* frame,
/// independent of what's already selected — [`selection::click_selection_system`]
/// is what actually commits this preview into [`SelectionState`] on click
/// (respecting Ctrl/Shift to add another group, or Alt to remove this one),
/// so there's no separate click-time seed to track here.
pub(crate) fn update_hover_preview_group(
    hover:        Res<HoverResult>,
    planar:       Res<PlanarSelectionSettings>,
    model:        Option<Res<FemModel>>,
    slider_query: Query<&SliderState, With<SliderTrack>>,
    mut preview:  ResMut<fem_core::HoverPreviewTargets>,
) {
    let new_targets: Vec<fem_core::FemEntityId> = match hover.target {
        None => Vec::new(),

        Some(target) if !planar.enabled => vec![target],

        Some(target) => {
            let threshold = slider_query.iter()
                .find(|s| s.id == SliderId::PlanarAngle)
                .map(|s| s.value)
                .unwrap_or(planar.angle_deg);

            let Some(mesh) = model.as_deref().and_then(|m| m.meshes.first()) else {
                preview.targets = vec![target];
                return;
            };

            match target {
                fem_core::FemEntityId::Face(fid) => {
                    let (faces, _) = fem_core::expand_coplanar_from_face(mesh, fid, threshold);
                    faces.into_iter().map(fem_core::FemEntityId::Face).collect()
                }
                fem_core::FemEntityId::Element(eid) => {
                    // Always commit `Element` targets here, matching the
                    // seed's own kind — never `Face`. `expand_coplanar_from_element`
                    // returns `faces` only as an internal detail of how it
                    // computed the group (it walks by face, since that's
                    // where normals live); rendering already resolves an
                    // `Element` target to one of its own boundary faces via
                    // `find_boundary_face_for_target`, so there was never a
                    // need to swap the *committed* target's kind to `Face`
                    // for display purposes. Doing so was a real bug: in
                    // Element selection mode, it silently mixed `Face`
                    // targets into what should be a pure `Element`
                    // selection, corrupting anything downstream that
                    // switches on target kind (element-group export,
                    // DLOAD/BC element counts, ...).
                    let (_, elements) = fem_core::expand_coplanar_from_element(mesh, eid, threshold);
                    elements.into_iter().map(fem_core::FemEntityId::Element).collect()
                }
                other => vec![other],
            }
        }
    };

    // Avoid marking the resource `Changed` (and so triggering a highlight
    // mesh rebuild) every single frame when nothing actually moved.
    if preview.targets != new_targets {
        preview.targets = new_targets;
    }
}

/// Updates the `SelectionInfoText` every frame with:
///   "37 nodes selected  ·  Hover: Node 412 (x=12.3, y=0.0, z=-5.1)"
///
/// This is the single most important piece of contextual feedback in the
/// pre-process workflow: a person must know *what is selected* before
/// clicking a boundary-condition or load preset.
pub(crate) fn update_selection_info_text(
    selection:  Res<SelectionState>,
    hover:      Res<HoverResult>,
    model:      Option<Res<FemModel>>,
    mut query:  Query<&mut Text, With<SelectionInfoText>>,
) {
    let Ok(mut text) = query.single_mut() else { return; };

    // Count selected entities by type.
    let node_count = selection.targets.iter().filter(|t| matches!(t, fem_core::FemEntityId::Node(_))).count();
    let elem_count = selection.targets.iter().filter(|t| matches!(t, fem_core::FemEntityId::Element(_))).count();

    let sel_part = match (node_count, elem_count) {
        (0, 0) => "Nothing selected".to_string(),
        (n, 0) => format!("{n} node{} selected", if n == 1 { "" } else { "s" }),
        (0, e) => format!("{e} element{} selected", if e == 1 { "" } else { "s" }),
        (n, e) => format!("{n} node{}, {e} elem{} selected",
            if n==1{""} else {"s"}, if e==1{""} else {"s"}),
    };

    // Hover info: show node XYZ when hovering a node.
    let hover_part = hover.target.and_then(|target| {
        let fem_core::FemEntityId::Node(node_id) = target else { return None; };
        model.as_deref()?.meshes.iter().find_map(|mesh| {
            mesh.node_position(node_id).map(|pos| {
                format!("  |  Node {} ({:.3}, {:.3}, {:.3})", node_id.0, pos.x, pos.y, pos.z)
            })
        })
    }).unwrap_or_default();

    **text = format!("{sel_part}{hover_part}");
}

/// Updates the text inside each [`ConstraintPresetButton`] to show the
/// current selected-node count, e.g. "Fix XYZ (37)".
/// When 0 nodes are selected, the button is dimmed.
pub(crate) fn update_constraint_button_labels(
    selection:  Res<SelectionState>,
    buttons:    Query<(&ConstraintPresetButton, &Children), Without<ConstraintPresetLabel>>,
    mut labels: Query<&mut Text, With<ConstraintPresetLabel>>,
) {
    if !selection.is_changed() { return; }

    let n = selection.targets.iter()
        .filter(|t| matches!(t, fem_core::FemEntityId::Node(_)))
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
    selection:    Res<SelectionState>,
    selected_dir: Res<SelectedLoadDirection>,
    slider_query: Query<&SliderState, With<SliderTrack>>,
    mut labels:   Query<&mut Text, With<ApplyLoadLabel>>,
) {
    if !selection.is_changed() && !selected_dir.is_changed() { return; }

    let Ok(mut text) = labels.single_mut() else { return; };

    let n = selection.targets.iter()
        .filter(|t| matches!(t, fem_core::FemEntityId::Node(_)))
        .count();

    let mag = slider_query.iter()
        .find(|s| s.id == SliderId::LoadMagnitude)
        .map(|s| s.value)
        .unwrap_or(100.0);

    let dir_label = selected_dir.0.map(|(dof, sign)| {
        let axis = ["?","X","Y","Z"].get(dof as usize).copied().unwrap_or("?");
        let sign_char = if sign >= 0.0 { "+" } else { "-" };
        format!(" {sign_char}{axis} {mag:.0}")
    }).unwrap_or_else(|| " (pick direction)".to_string());

    **text = if n > 0 {
        format!("Apply Load{dir_label}  ({n} nodes)")
    } else {
        format!("Apply Load{dir_label}  - no nodes selected")
    };
}

/// Clears all boundary conditions and nodal loads at once.
pub(crate) fn clear_all_bc_loads_button_system(
    mut setup:   ResMut<fem_core::AnalysisSetup>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor),
        With<ClearAllBcLoadsButton>,
    >,
) {
    for (interaction, mut bg) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            setup.boundary_conditions.clear();
            setup.nodal_loads.clear();
            setup.distributed_loads.clear();
            setup.set_changed();
        }

        *bg = BackgroundColor(match *interaction {
            Interaction::Pressed | Interaction::Hovered =>
                Color::srgba(0.60, 0.15, 0.15, 0.95),
            Interaction::None =>
                Color::srgba(0.40, 0.12, 0.12, 0.80),
        });
    }
}

pub(crate) fn constraint_preset_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    model: Option<Res<FemModel>>,
    selection: Res<SelectionState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor, &ConstraintPresetButton),
        With<ConstraintPresetButton>,
    >,
) {
    let Some(model) = model else { return; };

    for (interaction, mut bg, mut border, preset) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let nodes: Vec<fem_core::NodeId> = selection.targets.iter().filter_map(|target| {
                if let fem_core::FemEntityId::Node(id) = target { Some(*id) } else { None }
            }).collect();

            if nodes.is_empty() { continue; }

            let mesh_index = model.meshes.iter().enumerate().find_map(|(i, mesh)| {
                nodes.iter().all(|&node| mesh.node_position(node).is_some()).then_some(i)
            }).unwrap_or(0);

            // Compute the name *before* the mutable push — Rust cannot hold
            // an immutable borrow (for the name lookup) and a mutable borrow
            // (for `push`) on `setup` at the same time within a single
            // struct expression.
            let bc_name = setup.next_auto_name_pub("BC");

            setup.boundary_conditions.push(fem_core::BoundaryCondition {
                name: bc_name,
                mesh_index,
                nodes,
                ngrp_name: None, // created from selection, not from a NGRP
                dof_start: preset.dof_start,
                dof_end: preset.dof_end,
                value: 0.0,
            });
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
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor, &LoadDirectionButton),
        With<LoadDirectionButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let new_dir = (btn.dof, btn.sign);
            if selected.0 == Some(new_dir) {
                selected.0 = None; // toggle off
            } else {
                selected.0 = Some(new_dir);
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
    let Some(model) = model else { return; };

    let magnitude = slider_query.iter()
        .find(|s| s.id == SliderId::LoadMagnitude)
        .map(|s| s.value)
        .unwrap_or(100.0);

    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let Some((dof, sign)) = selected_dir.0 else { continue; };

            let nodes: Vec<fem_core::NodeId> = selection.targets.iter().filter_map(|target| {
                if let fem_core::FemEntityId::Node(id) = target { Some(*id) } else { None }
            }).collect();

            if nodes.is_empty() { continue; }

            let mesh_index = model.meshes.iter().enumerate().find_map(|(i, mesh)| {
                nodes.iter().all(|&n| mesh.node_position(n).is_some()).then_some(i)
            }).unwrap_or(0);

            let name = setup.next_auto_name_pub("LOAD");
            let value = magnitude * sign;

            for node in nodes {
                setup.nodal_loads.push(fem_core::NodalLoad {
                    name: name.clone(),
                    mesh_index,
                    node,
                    ngrp_name: None, // created from selection, not from a NGRP
                    dof,
                    value,
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

/// Adds one of the built-in material presets to [`AnalysisSetup`]. If a
/// material with the same name already exists the button is a no-op (to
/// avoid duplicate entries cluttering the list).
/// Toggles the active [`SelectedDloadKind`] when [Pressure]/[Gravity] clicked.
pub(crate) fn dload_kind_button_system(
    mut selected: ResMut<SelectedDloadKind>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor, &DloadKindButton),
        With<DloadKindButton>,
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
) -> Vec<fem_core::ElementId> {
    let face_ids: Vec<fem_core::FaceId> = selection.targets.iter().filter_map(|t| {
        if let fem_core::FemEntityId::Face(id) = t { Some(*id) } else { None }
    }).collect();

    if !face_ids.is_empty() {
        let mut elements = Vec::new();
        for mesh in &model.meshes {
            for face in mesh.cached_boundary_faces() {
                if face_ids.contains(&face.id) {
                    if let Some(eid) = face.element {
                        if !elements.contains(&eid) { elements.push(eid); }
                    }
                }
            }
        }
        return elements;
    }

    // Fallback: directly-selected elements.
    selection.targets.iter().filter_map(|t| {
        if let fem_core::FemEntityId::Element(id) = t { Some(*id) } else { None }
    }).collect()
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
/// Like [`selected_elements_from_faces_or_elements`], this scans every
/// mesh in the model rather than tracking which mesh a selected id belongs
/// to — [`SelectionState`] doesn't carry a mesh index today, so a
/// multi-part assembly with colliding face ids across parts is a known
/// limitation shared with the rest of the selection system, not something
/// newly introduced here.
fn selected_faces_from_faces_or_elements(
    selection: &SelectionState,
    model: &FemModel,
) -> Vec<fem_core::ElementFaceRef> {
    model
        .meshes
        .iter()
        .flat_map(|mesh| mesh.surface_refs_from_targets(&selection.targets))
        .collect()
}

/// Updates the [`ApplyDloadButton`]'s label with the current face/element
/// count, mirroring [`update_apply_load_label`]'s feedback pattern.
pub(crate) fn update_apply_dload_label(
    selection: Res<SelectionState>,
    model:     Option<Res<FemModel>>,
    kind:      Res<SelectedDloadKind>,
    slider_query: Query<&SliderState, With<SliderTrack>>,
    mut labels: Query<&mut Text, With<ApplyDloadLabel>>,
) {
    if !selection.is_changed() && !kind.is_changed() { return; }

    let Ok(mut text) = labels.single_mut() else { return; };
    let Some(model) = model.as_deref() else { return; };

    // Pressure counts picked *faces* (what actually gets written to the
    // .cnt); gravity counts elements, since it has no face to speak of.
    let (n, unit) = match *kind {
        SelectedDloadKind::Pressure => (
            selected_faces_from_faces_or_elements(&selection, model).len(),
            "faces",
        ),
        SelectedDloadKind::Gravity => (
            selected_elements_from_faces_or_elements(&selection, model).len(),
            "elements",
        ),
    };

    let mag = slider_query.iter()
        .find(|s| s.id == SliderId::DloadMagnitude)
        .map(|s| s.value)
        .unwrap_or(1.0);

    let kind_label = match *kind { SelectedDloadKind::Pressure => "Pressure", SelectedDloadKind::Gravity => "Gravity" };

    **text = if n > 0 {
        format!("Apply {kind_label} {mag:.2}  ({n} {unit})")
    } else {
        format!("Apply {kind_label}  - no faces/elements selected")
    };
}

/// Creates a [`fem_core::DistributedLoad`] from the currently selected faces
/// (resolved to their parent elements) and the configured kind/magnitude.
pub(crate) fn apply_dload_button_system(
    mut setup:    ResMut<fem_core::AnalysisSetup>,
    model:        Option<Res<FemModel>>,
    selection:    Res<SelectionState>,
    kind:         Res<SelectedDloadKind>,
    slider_query: Query<&SliderState, With<SliderTrack>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ApplyDloadButton>,
    >,
) {
    let Some(model) = model.as_deref() else { return; };

    let magnitude = slider_query.iter()
        .find(|s| s.id == SliderId::DloadMagnitude)
        .map(|s| s.value)
        .unwrap_or(1.0);

    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            // Pressure needs which face was picked (P1..P6 in the exported
            // .cnt); gravity is a whole-element body force and has no face.
            let (dload_kind, target) = match *kind {
                SelectedDloadKind::Pressure => (
                    fem_core::DistributedLoadKind::Pressure,
                    fem_core::DistributedLoadTarget::Faces(
                        selected_faces_from_faces_or_elements(&selection, model),
                    ),
                ),
                SelectedDloadKind::Gravity => (
                    fem_core::DistributedLoadKind::Gravity,
                    fem_core::DistributedLoadTarget::Elements(
                        selected_elements_from_faces_or_elements(&selection, model),
                    ),
                ),
            };

            if !target.is_empty() {
                let name = setup.next_auto_name_pub("DLOAD");

                setup.distributed_loads.push(fem_core::DistributedLoad {
                    name,
                    mesh_index: 0,
                    target,
                    kind: dload_kind,
                    value: magnitude,
                });
                setup.set_changed();
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

pub(crate) fn analysis_type_button_system(
    mut setup:   ResMut<fem_core::AnalysisSetup>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor, &AnalysisTypeButton),
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
    mut setup:   ResMut<fem_core::AnalysisSetup>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor, &SolverMethodButton),
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
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor, &MaterialPresetButton),
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
    parent.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6.0),
            ..default()
        },
        Name::new(name.to_string()),
    )).with_children(|row| {
        row.spawn((
            Text::new(label.to_string()),
            TextFont { font_size: FontSize::Px(10.0), ..default() },
            TextColor(TEXT_MAIN),
            Node { flex_grow: 1.0, ..default() },
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
        )).with_child((
            Text::new("x"),
            TextFont { font_size: FontSize::Px(9.0), ..default() },
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
            TextFont { font_size: FontSize::Px(10.5), ..default() },
            TextColor(TEXT_MAIN),
        ));
}

/// Selects every member of the clicked set, mirroring the result of a box
/// select over exactly that set: clears the current selection (or extends
/// it with Ctrl held), sets [`SelectionFilter::level`] to match the set's
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
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor, &SetButton),
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

                    if let Some(targets) = targets {
                        let ctrl = keyboard.pressed(KeyCode::ControlLeft)
                            || keyboard.pressed(KeyCode::ControlRight);

                        if !ctrl {
                            for entity in &selected_query {
                                commands.entity(entity).remove::<Selected>();
                            }
                            selection.clear();
                        }

                        filter.level = match set_button.kind {
                            SetKind::Node => SelectionLevel::Node,
                            SetKind::Element => SelectionLevel::Element,
                            SetKind::Surface => SelectionLevel::Face,
                        };

                        for target in &targets {
                            if !selection.targets.contains(target) {
                                selection.targets.push(*target);
                            }
                        }

                        for (entity, selectable) in &selectable_query {
                            if targets.contains(&selectable.target) {
                                commands.entity(entity).insert(Selected);

                                if !selection.entities.contains(&entity) {
                                    selection.entities.push(entity);
                                }
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

/// Lists each loaded [`fem_core::Part`] with its node/element counts,
/// one line per part, so an assembly built up via [`import_mesh_button_system`]
/// shows what's been added so far.
///
/// Hidden (empty text) when there's a single part, since the main mesh
/// stats text already covers that case.
pub(crate) fn update_parts_list_text(
    model: Option<Res<FemModel>>,
    mut query: Query<&mut Text, With<PartsListText>>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };

    let Some(model) = model else {
        **text = String::new();
        return;
    };

    if model.parts.len() <= 1 {
        **text = String::new();
        return;
    }

    let lines: Vec<String> = model
        .parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let counts = model
                .meshes
                .get(part.mesh_index)
                .map(|mesh| format!("{}N / {}E", mesh.nodes.len(), mesh.elements.len()))
                .unwrap_or_default();

            format!("  [{}] {}  ({counts})", index + 1, part.name)
        })
        .collect();

    **text = format!("Parts:\n{}", lines.join("\n"));
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
        .target
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

fn contact_candidate_summary(state: &ContactCandidateState, model: Option<&FemModel>) -> String {
    let total = state.candidates.len();

    let Some(candidate) = state.selected_candidate() else {
        return if total == 0 {
            "Contacts: no candidates (press Detect)".to_string()
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
        .and_then(|model| model.parts.iter().find(|part| part.mesh_index == mesh_index))
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
            Interaction::None    => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border     = BorderColor::all(PANEL_BORDER);
    }

    // Load on a separate branch to avoid holding rfd dialog open
    // while mutating FemResultSet.
    if let Some(path) = pending_path.take() {
        let Some(model)    = model.as_deref()       else { return; };
        let Some(fem_mesh) = model.meshes.first()   else { return; };

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
            "frd" => {
                match hecmw::load_frd_file(&path, &node_ids) {
                    Ok(steps) => steps,
                    Err(err)  => { bevy::log::warn!("FRD load failed: {err}"); return; }
                }
            }
            "vtu" | "pvtu" => {
                match hecmw::load_vtu_file(&path, &node_ids) {
                    Ok(step)  => vec![step],
                    Err(err)  => { bevy::log::warn!("VTU load failed: {err}"); return; }
                }
            }
            _ => {
                // .res.0.N — auto-detect series siblings and load all steps.
                match hecmw::load_series(&path, &node_ids) {
                    Ok(steps) => steps,
                    Err(err)  => { bevy::log::warn!("Result series load failed: {err}"); return; }
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
                mesh_index:          active.mesh_index,
                step_index:          active.step_index,
                field_name:          active.field_name.clone(),
                show_deformation:    has_disp,
                displacement_field:  "Displacement".to_string(),
                deformation_scale:   1.0,
            });
        }

        bevy::log::info!("Loaded {step_count} result step(s) from {:?}", path.file_name());
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
    mut request:        ResMut<MeshLoadRequest>,
    mut load_status:    ResMut<MeshLoadStatus>,
    mut pending_cnt:    ResMut<fem_core::PendingCntLoad>,
    version:            Res<FemModelVersion>,
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
            Interaction::None    => Color::srgb(0.10, 0.30, 0.18),
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
    model:           Option<Res<FemModel>>,
    version:         Res<FemModelVersion>,
    mut pending_cnt: ResMut<fem_core::PendingCntLoad>,
    mut setup:       ResMut<fem_core::AnalysisSetup>,
) {
    if pending_cnt.path.is_none() {
        return;
    }

    let Some((path, mesh_index)) = pending_cnt.take_if_ready(version.value) else { return; };
    let Some(model) = model.as_deref() else { return; };
    let Some(mesh) = model.meshes.get(mesh_index) else { return; };

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
            );
            setup.boundary_conditions.extend(data.boundary_conditions);
            setup.nodal_loads.extend(data.nodal_loads);
            setup.distributed_loads.extend(data.distributed_loads);
            setup.materials.extend(data.materials);
            setup.sections.extend(data.sections);
            setup.set_changed();
            bevy::log::info!(
                "Loaded analysis setup from {:?}: {} BCs / {} constrained nodes, {} nodal loads, {} distributed loads, {} materials, {} sections",
                path.file_name(),
                counts.0,
                counts.1,
                counts.2,
                counts.3,
                counts.4,
                counts.5,
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
    mut model:     ResMut<FemModel>,
    selection:     Res<SelectionState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<MakeNodeGroupButton>,
    >,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let nodes: Vec<fem_core::NodeId> = selection.targets.iter().filter_map(|t| {
                if let fem_core::FemEntityId::Node(id) = t { Some(*id) } else { None }
            }).collect();

            if !nodes.is_empty() {
                let mesh = model.meshes.first_mut().unwrap();
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
    mut model:     ResMut<FemModel>,
    selection:     Res<SelectionState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<MakeElementGroupButton>,
    >,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let elements: Vec<fem_core::ElementId> = selection.targets.iter().filter_map(|t| {
                if let fem_core::FemEntityId::Element(id) = t { Some(*id) } else { None }
            }).collect();

            if !elements.is_empty() {
                let mesh = model.meshes.first_mut().unwrap();
                let n = mesh.element_sets.len() + 1;
                let name = format!("EGRP{n}");
                mesh.element_sets.push(fem_core::FemElementSet { name, elements });
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
    mut selected:  ResMut<SelectedSectionType>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor, &SectionTypeButton),
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
    mut selected:  ResMut<SelectedEgrp>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor, &EgrpSelectButton),
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
    mut selected:  ResMut<SelectedMaterialForSection>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor, &MaterialSelectButton),
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
    mut setup:        ResMut<fem_core::AnalysisSetup>,
    section_type:     Res<SelectedSectionType>,
    egrp:             Res<SelectedEgrp>,
    material_sel:     Res<SelectedMaterialForSection>,
    slider_query:     Query<&SliderState, With<SliderTrack>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<AddSectionButton>,
    >,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let Some(mat_name) = &material_sel.0 else { continue; };

            let thickness = slider_query.iter()
                .find(|s| s.id == SliderId::SectionThickness)
                .map(|s| s.value)
                .unwrap_or(2.0);

            let kind = match *section_type {
                SelectedSectionType::Solid => fem_core::SectionKind::Solid,
                SelectedSectionType::Shell => fem_core::SectionKind::Shell { thickness },
                SelectedSectionType::Beam  => fem_core::SectionKind::Beam  { area: thickness },
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
    mut commands:  Commands,
    model:         Option<Res<FemModel>>,
    setup:         Res<fem_core::AnalysisSetup>,
    version:       Res<FemModelVersion>,
    mut last_ver:  Local<Option<u64>>,
    egrp_row_q:    Query<Entity, With<SectionDefEgrpRow>>,
    mat_row_q:     Query<Entity, With<SectionDefMatRow>>,
    children_q:    Query<&Children>,
) {
    let ver_changed = *last_ver != Some(version.value);
    *last_ver = Some(version.value);

    if !ver_changed && !setup.is_changed() {
        return;
    }

    // ── EGRP buttons ──
    if let Ok(egrp_row) = egrp_row_q.single() {
        if let Ok(children) = children_q.get(egrp_row) {
            for &c in children { commands.entity(c).despawn(); }
        }

        commands.entity(egrp_row).with_children(|row| {
            // "ALL" option
            row.spawn((
                Button,
                Node { padding: UiRect::axes(px(8.0), px(3.0)), border: UiRect::all(px(1.0)), border_radius: BorderRadius::all(px(4.0)), ..default() },
                BackgroundColor(BUTTON_NORMAL), BorderColor::all(PANEL_BORDER),
                EgrpSelectButton(None),
                Name::new("Egrp_ALL"),
            )).with_child((Text::new("ALL"), TextFont { font_size: FontSize::Px(9.0), ..default() }, TextColor(TEXT_MAIN)));

            if let Some(model) = model.as_deref() {
                for mesh in &model.meshes {
                    for eset in &mesh.element_sets {
                        let name = eset.name.clone();
                        row.spawn((
                            Button,
                            Node { padding: UiRect::axes(px(8.0), px(3.0)), border: UiRect::all(px(1.0)), border_radius: BorderRadius::all(px(4.0)), ..default() },
                            BackgroundColor(BUTTON_NORMAL), BorderColor::all(PANEL_BORDER),
                            EgrpSelectButton(Some(name.clone())),
                            Name::new(format!("Egrp_{name}")),
                        )).with_child((Text::new(name), TextFont { font_size: FontSize::Px(9.0), ..default() }, TextColor(TEXT_MAIN)));
                    }
                }
            }
        });
    }

    // ── Material buttons ──
    if let Ok(mat_row) = mat_row_q.single() {
        if let Ok(children) = children_q.get(mat_row) {
            for &c in children { commands.entity(c).despawn(); }
        }

        commands.entity(mat_row).with_children(|row| {
            for mat in &setup.materials {
                let name = mat.name.clone();
                row.spawn((
                    Button,
                    Node { padding: UiRect::axes(px(8.0), px(3.0)), border: UiRect::all(px(1.0)), border_radius: BorderRadius::all(px(4.0)), ..default() },
                    BackgroundColor(BUTTON_NORMAL), BorderColor::all(PANEL_BORDER),
                    MaterialSelectButton(name.clone()),
                    Name::new(format!("MatSel_{name}")),
                )).with_child((Text::new(name), TextFont { font_size: FontSize::Px(9.0), ..default() }, TextColor(TEXT_MAIN)));
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

            let stem = status.last_path.as_deref()
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
                Err(e)  => set_export_status(&mut status_query, &format!("Error: {e}")),
            }
        }

        let color = match *interaction {
            Interaction::Pressed => Color::srgb(0.12, 0.44, 0.22),
            Interaction::Hovered => Color::srgb(0.14, 0.52, 0.26),
            Interaction::None    => Color::srgb(0.10, 0.32, 0.18),
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
    model: Option<Res<FemModel>>,
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
            Interaction::None    => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border     = BorderColor::all(PANEL_BORDER);
    }

    if let Some(path) = pending_path.take() {
        let Some(model) = model.as_deref() else { return; };
        let Some(mesh) = model.meshes.first() else { return; };

        match hecmw::load_cnt_file(&path, mesh, 0) {
            Ok(data) => {
                setup.boundary_conditions.extend(data.boundary_conditions);
                setup.nodal_loads.extend(data.nodal_loads);
                setup.distributed_loads.extend(data.distributed_loads);
                setup.materials.extend(data.materials);
                setup.sections.extend(data.sections);

                // Touch the resource so `is_changed()` consumers (e.g.
                // `update_analysis_setup_stats_text`) fire even if every
                // `extend` above happened to add zero items.
                setup.set_changed();

                bevy::log::info!("Loaded analysis setup from {:?}", path.file_name());
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
            fem_core::ResultField::NodeVector { name, min_mag, max_mag, .. } => {
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
    mut playback:    ResMut<PlaybackState>,
    results:         Option<Res<FemResultSet>>,
    mut play_btns:   Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor, &Children),
        (With<PlaybackPlayPauseButton>, Without<PlaybackRewindButton>, Without<PlaybackEndButton>),
    >,
    mut rewind_btns: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        (With<PlaybackRewindButton>, Without<PlaybackPlayPauseButton>, Without<PlaybackEndButton>),
    >,
    mut end_btns:    Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        (With<PlaybackEndButton>, Without<PlaybackPlayPauseButton>, Without<PlaybackRewindButton>),
    >,
    mut labels:      Query<&mut Text, With<PlaybackPlayPauseLabel>>,
    mut sliders:     Query<&mut SliderState, With<SliderTrack>>,
) {
    let step_count = results.as_deref().map(|r| r.by_mesh.iter().map(|s| s.len()).max().unwrap_or(0)).unwrap_or(0);

    for (interaction, mut bg, mut border, children) in &mut play_btns {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            playback.playing = !playback.playing;
            playback.elapsed  = 0.0;
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
                if s.id == SliderId::ResultStep { s.value = 0.0; s.clamp_value(); }
            }
        }
        *bg = BackgroundColor(if *interaction != Interaction::None { BUTTON_HOVERED } else { BUTTON_NORMAL });
        *border = BorderColor::all(PANEL_BORDER);
    }

    for (interaction, mut bg, mut border) in &mut end_btns {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            playback.playing = false;
            let last = (step_count.saturating_sub(1)) as f32;
            for mut s in &mut sliders {
                if s.id == SliderId::ResultStep { s.value = last; s.clamp_value(); }
            }
        }
        *bg = BackgroundColor(if *interaction != Interaction::None { BUTTON_HOVERED } else { BUTTON_NORMAL });
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Advances the result step automatically when [`PlaybackState::playing`]
/// is true, using [`PlaybackState::interval`] as the seconds-per-step.
/// Wraps back to step 0 when the last step is reached (loop mode).
pub(crate) fn playback_advance_system(
    time:           Res<Time>,
    mut playback:   ResMut<PlaybackState>,
    results:        Option<Res<FemResultSet>>,
    mut sliders:    Query<&mut SliderState, With<SliderTrack>>,
) {
    if !playback.playing { return; }

    let step_count = results.as_deref().map(|r| r.by_mesh.iter().map(|s| s.len()).max().unwrap_or(0)).unwrap_or(0);
    if step_count == 0 { playback.playing = false; return; }

    // Read speed from slider
    let speed = sliders.iter().find(|s| s.id == SliderId::PlaybackSpeed)
        .map(|s| s.value).unwrap_or(2.0);
    playback.interval = 1.0 / speed.max(0.1);

    playback.elapsed += time.delta_secs();
    if playback.elapsed < playback.interval { return; }
    playback.elapsed = 0.0;

    for mut s in &mut sliders {
        if s.id != SliderId::ResultStep { continue; }
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
    keys:           Res<ButtonInput<KeyCode>>,
    mut setup:      ResMut<fem_core::AnalysisSetup>,
    mut stack:      ResMut<UndoStack>,
    mut in_progress: ResMut<UndoInProgress>,
) {
    let ctrl  = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft,   KeyCode::ShiftRight]);

    if ctrl && keys.just_pressed(KeyCode::KeyZ) && !shift {
        if let Some(prev) = stack.undo.pop() {
            let current = std::mem::replace(&mut *setup, prev);
            stack.redo.push(current);
            in_progress.0 = true;
        }
    }

    if ctrl && (keys.just_pressed(KeyCode::KeyY)
        || (shift && keys.just_pressed(KeyCode::KeyZ)))
    {
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
    setup:           Res<fem_core::AnalysisSetup>,
    mut stack:       ResMut<UndoStack>,
    mut in_progress: ResMut<UndoInProgress>,
    mut prev:        Local<Option<fem_core::AnalysisSetup>>,
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
    results: Res<FemResultSet>,
    mut slider_query: Query<&mut SliderState, With<SliderTrack>>,
) {
    if !results.has_results() {
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
    let mut step_value: Option<f32>  = None;
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
            SliderId::LoadMagnitude | SliderId::SectionThickness | SliderId::PlanarAngle
                | SliderId::DloadMagnitude | SliderId::PlaybackSpeed => {}
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
                contour.step_index        = step_index;
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

/// Formats a [`FemEntityId`] for status-line display.
///
/// For elements, appends the FEM element type (e.g. `"Element 354 (Hex8)"`)
/// when `model` is available — this makes mixed element-type meshes
/// self-diagnosing: if an element's rendered shape looks inconsistent with
/// its neighbours (e.g. a solid cuboid surrounded by thin shell plates),
/// hovering it immediately shows whether that's actually a different
/// element type rather than a rendering bug.
fn entity_label(target: FemEntityId, model: Option<&FemModel>) -> String {
    match target {
        FemEntityId::Node(id) => format!("Node {}", id.0),
        FemEntityId::Edge(id) => format!("Edge {}", id.0),
        FemEntityId::Face(id) => format!("Face {}", id.0),
        FemEntityId::Element(id) => {
            let type_label = model
                .and_then(|model| model.meshes.iter().find_map(|mesh| {
                    mesh.elements.iter().find(|e| e.id == id).map(|e| element_type_label(&e.element_type))
                }));

            match type_label {
                Some(label) => format!("Element {} ({label})", id.0),
                None => format!("Element {}", id.0),
            }
        }
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

    use super::{SidebarPage, SidebarPageContent, apply_mesh};
    use fem_core::{
        AnalysisSetup, FemMesh, FemModel, FemModelVersion, MeshLoadStatus, NodeId,
    };

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
    fn replacing_a_mesh_clears_setup_inside_the_load_transaction() {
        let mut model = FemModel::demo_hex8();
        let mut setup = AnalysisSetup::default();
        setup.add_constraint(0, vec![NodeId(0)], 1, 3, 0.0);
        let mut status = MeshLoadStatus::default();
        let mut version = FemModelVersion::default();

        apply_mesh(
            FemMesh::demo_hex8(),
            &PathBuf::from("replacement.msh"),
            false,
            &mut model,
            &mut status,
            &mut version,
            &mut setup,
        );

        assert!(setup.is_empty());
        assert_eq!(version.value, 1);
    }

    #[test]
    fn importing_an_assembly_part_preserves_existing_setup() {
        let mut model = FemModel::demo_hex8();
        let mut setup = AnalysisSetup::default();
        setup.add_constraint(0, vec![NodeId(0)], 1, 3, 0.0);
        let mut status = MeshLoadStatus::default();
        let mut version = FemModelVersion::default();

        apply_mesh(
            FemMesh::demo_hex8(),
            &PathBuf::from("part.msh"),
            true,
            &mut model,
            &mut status,
            &mut version,
            &mut setup,
        );

        assert_eq!(setup.boundary_conditions.len(), 1);
        assert_eq!(model.meshes.len(), 2);
    }
}
