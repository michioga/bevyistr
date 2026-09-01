use crate::assembly::{
    AssemblyEditorState, AssemblyGizmoMode, reference_size as assembly_reference_size,
};
use crate::bc_loads_ui::{BoundaryLoadsListContainer, spawn_boundary_load_editor};
use crate::contact_ui::{
    AcceptContactButton, AcceptContactLabel, CaptureContactSideButton, ContactBehaviorButton,
    ContactCandidateAction, ContactCandidateActionButton, ContactCandidateText, ContactCaptureSide,
    ContactDefinitionsListContainer, ContactDefinitionsText, ContactDraftStatusText,
    ContactGhostToggleButton, ContactGhostToggleLabel, ContactPairKind, ContactPairKindButton,
    ContactParameter, ContactParameterButton, ContactPenaltyControls, ContactPenaltyToggleButton,
    ContactPenaltyToggleLabel, ContactReviewControls, ContactSlidingParameterControls,
    DetectContactsButton, FinalizeContactButton,
};
use crate::materials_ui::spawn_materials_ui;
use crate::measurement::MeasurementBoxState;
use crate::mpc_ui::{
    AcceptRigidSpiderButton, CaptureMpcPairNodeButton, ClearMpcPairButton, CreateMpcPairButton,
    DefinedMpcAction, DefinedMpcActionButton, DefinedMpcText, DetectRigidSpidersButton,
    MpcPairDofButton, MpcPairDraftText, MpcPairSide, RigidSpiderAction, RigidSpiderActionButton,
    RigidSpiderCandidateText,
};
use crate::project_io::{ImportMeshButton, OpenMeshButton, OpenProjectButton};
use crate::results_ui::{
    OpenResultButton, PlaybackEndButton, PlaybackPlayPauseButton, PlaybackPlayPauseLabel,
    PlaybackRewindButton, ResultSliderSection, ResultStatsText,
};
use crate::selection_ui::{
    DEFAULT_SMOOTH_ANGLE_DEG, MakeElementGroupButton, MakeNodeGroupButton, SELECTION_GUIDE_TEXT,
    SelectionContextText, SelectionGuidePanel, SelectionGuideToggle, SelectionInfoText,
    SelectionLevelButton, SelectionOperationHint, SelectionStatsText, SelectionToolbar,
    SurfaceAngleControls, SurfaceSelectionControls, SurfaceSelectionHint, SurfaceSelectionMode,
    SurfaceSelectionModeButton, SurfaceSelectionUnavailableHint,
};
use crate::slider::{SliderConfig, SliderId, SliderState, SliderTrack, spawn_slider};
use crate::solve_ui::spawn_solve_ui;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::ScrollPosition;
use fem_core::{
    ContactCandidateState, ContactType, FemModel, FemModelVersion, MeshLoadStatus, SelectionFilter,
    SelectionLevel, UiPointerState, ViewportTool,
};
use interaction::HoverResult;
use selection::{Selectable, Selected, SelectionState};
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
pub(crate) struct RenderModeButton {
    mode: VisualizationMode,
}

#[derive(Component)]
pub(crate) struct MeshStatsText;

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

    pub(crate) const fn page(page: SidebarPage) -> Self {
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
                spawn_slider(sec, SliderConfig {
                    width: 272.0,
                    min: 0.0,
                    max: 10.0,
                    value: 0.05,
                    label: "Search gap (model units)",
                    id: SliderId::ContactSearchGap,
                });
                action_button(
                    sec,
                    "Edit search gap exactly",
                    ContactParameterButton(ContactParameter::SearchGap),
                    "EditContactSearchGapButton",
                );
                spawn_slider(sec, SliderConfig {
                    width: 272.0,
                    min: 0.0,
                    max: 90.0,
                    value: 20.0,
                    label: "Normal tolerance (deg)",
                    id: SliderId::ContactSearchAngle,
                });
                action_button(
                    sec,
                    "Edit normal tolerance exactly",
                    ContactParameterButton(ContactParameter::SearchAngle),
                    "EditContactSearchAngleButton",
                );
                action_button(
                    sec,
                    "Detect Contact Candidates",
                    DetectContactsButton,
                    "DetectContactsButton",
                );
                hint_text(sec, "Surface gap search; coarse side = Master, fine side = Slave");
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
                            AcceptContactButton,
                            Name::new("AcceptContactButton"),
                        ))
                        .with_child((
                            Text::new("Accept as Small sliding"),
                            TextFont {
                                font_size: FontSize::Px(11.5),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                            AcceptContactLabel,
                        ));
                    });
                });
                hint_text(
                    sec,
                    "Accept uses BEHAVIOR and friction settings above; review separation is display only",
                );
            });
            divider(panel);

            // ── § MPC / rigid spider ────────────────────────────────────
            section(panel, "MPC / RIGID SPIDER", |sec| {
                sec.spawn((
                    Text::new("PAIR MPC FROM VIEWPORT"),
                    TextFont {
                        font_size: FontSize::Px(9.5),
                        ..default()
                    },
                    TextColor(Color::srgba(0.44, 0.60, 0.72, 0.90)),
                ));
                hint_text(sec, "Select exactly one Node, then capture each side; reference = magenta (+), coupled = cyan (-)");
                sec.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6.0),
                    ..default()
                },))
                    .with_children(|row| {
                        action_button(
                            row,
                            "1  Reference (+)",
                            CaptureMpcPairNodeButton(MpcPairSide::Positive),
                            "CaptureMpcPairPositiveButton",
                        );
                        action_button(
                            row,
                            "2  Coupled (-)",
                            CaptureMpcPairNodeButton(MpcPairSide::Negative),
                            "CaptureMpcPairNegativeButton",
                        );
                    });
                sec.spawn((
                    Text::new("Reference: not set   Coupled: not set"),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                    MpcPairDraftText,
                ));
                sec.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4.0),
                    ..default()
                },))
                    .with_children(|row| {
                        for (dof, label, name) in [
                            (0, "XYZ", "MpcPairDofXyzButton"),
                            (1, "Ux", "MpcPairDofXButton"),
                            (2, "Uy", "MpcPairDofYButton"),
                            (3, "Uz", "MpcPairDofZButton"),
                        ] {
                            action_button(row, label, MpcPairDofButton(dof), name);
                        }
                    });
                sec.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6.0),
                    ..default()
                },))
                    .with_children(|row| {
                        action_button(
                            row,
                            "Clear",
                            ClearMpcPairButton,
                            "ClearMpcPairButton",
                        );
                        action_button(
                            row,
                            "3  Create !EQUATION",
                            CreateMpcPairButton,
                            "CreateMpcPairButton",
                        );
                    });
                hint_text(sec, "XYZ creates three grouped equations; exact constants and coefficients remain editable below");
                divider(sec);
                sec.spawn((
                    Text::new("AUTOMATIC RIGID SPIDER"),
                    TextFont {
                        font_size: FontSize::Px(9.5),
                        ..default()
                    },
                    TextColor(Color::srgba(0.44, 0.60, 0.72, 0.90)),
                ));
                spawn_slider(sec, SliderConfig {
                    width: 272.0,
                    min: 0.0,
                    max: 20.0,
                    value: 1.0,
                    label: "Search radius (model units)",
                    id: SliderId::RigidSpiderRadius,
                });
                action_button(
                    sec,
                    "Edit radius exactly",
                    ContactParameterButton(ContactParameter::SpiderRadius),
                    "EditRigidSpiderRadiusButton",
                );
                action_button(
                    sec,
                    "Detect MPC Spiders",
                    DetectRigidSpidersButton,
                    "DetectRigidSpidersButton",
                );
                sec.spawn((
                    Text::new("No MPC candidates — run Detect MPC Spiders"),
                    TextFont { font_size: FontSize::Px(10.5), ..default() },
                    TextColor(TEXT_MUTED),
                    RigidSpiderCandidateText,
                ));
                sec.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6.0),
                    ..default()
                },)).with_children(|row| {
                    action_button(
                        row,
                        "Previous",
                        RigidSpiderActionButton(RigidSpiderAction::Previous),
                        "PreviousRigidSpiderButton",
                    );
                    action_button(
                        row,
                        "Next",
                        RigidSpiderActionButton(RigidSpiderAction::Next),
                        "NextRigidSpiderButton",
                    );
                });
                sec.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6.0),
                    ..default()
                },)).with_children(|row| {
                    action_button(
                        row,
                        "Reject",
                        RigidSpiderActionButton(RigidSpiderAction::Reject),
                        "RejectRigidSpiderButton",
                    );
                    action_button(
                        row,
                        "Create !EQUATION",
                        AcceptRigidSpiderButton,
                        "AcceptRigidSpiderButton",
                    );
                });
                hint_text(sec, "Center = magenta, solid boundary nodes = cyan; isolated centers transfer translations only");
                divider(sec);
                sec.spawn((
                    Text::new("DEFINED MPC REVIEW"),
                    TextFont { font_size: FontSize::Px(9.5), ..default() },
                    TextColor(Color::srgba(0.44, 0.60, 0.72, 0.90)),
                ));
                sec.spawn((
                    Text::new("No MPC equations defined"),
                    TextFont { font_size: FontSize::Px(10.5), ..default() },
                    TextColor(TEXT_MUTED),
                    DefinedMpcText,
                ));
                action_button(
                    sec,
                    "Show selected in viewport",
                    DefinedMpcActionButton(DefinedMpcAction::Show),
                    "ShowDefinedMpcButton",
                );
                sec.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6.0),
                    ..default()
                },)).with_children(|row| {
                    action_button(
                        row,
                        "Previous equation",
                        DefinedMpcActionButton(DefinedMpcAction::Previous),
                        "PreviousDefinedMpcButton",
                    );
                    action_button(
                        row,
                        "Next equation",
                        DefinedMpcActionButton(DefinedMpcAction::Next),
                        "NextDefinedMpcButton",
                    );
                });
                action_button(
                    sec,
                    "Edit constant exactly",
                    DefinedMpcActionButton(DefinedMpcAction::EditConstant),
                    "EditDefinedMpcConstantButton",
                );
                sec.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6.0),
                    ..default()
                },)).with_children(|row| {
                    action_button(
                        row,
                        "Previous term",
                        DefinedMpcActionButton(DefinedMpcAction::PreviousTerm),
                        "PreviousDefinedMpcTermButton",
                    );
                    action_button(
                        row,
                        "Next term",
                        DefinedMpcActionButton(DefinedMpcAction::NextTerm),
                        "NextDefinedMpcTermButton",
                    );
                });
                action_button(
                    sec,
                    "Edit selected coefficient exactly",
                    DefinedMpcActionButton(DefinedMpcAction::EditCoefficient),
                    "EditDefinedMpcCoefficientButton",
                );
                hint_text(sec, "Exact edit uses the lower-right value box; Enter applies, Esc cancels");
                action_button(
                    sec,
                    "Remove selected equation / group",
                    DefinedMpcActionButton(DefinedMpcAction::Remove),
                    "RemoveDefinedMpcButton",
                );
                hint_text(sec, "Expanded spiders are removed as one group; Ctrl+Z restores the change");
                hint_text(sec, "Selected equation: positive coefficients = magenta, negative = cyan");
            });
            divider(panel);

            }); // end ContactPage

            sidebar_page_group(panel, SidebarPageContent::analysis(), "AnalysisPages", |panel| {

            // ── § Analysis Setup (boundary conditions / loads / materials) ──
            section(panel, "ANALYSIS SETUP", |sec| {
                // Export row: Open Setup + Export to FrontISTR on the same row
                spawn_solve_ui(sec);
                spawn_boundary_load_editor(sec);

                spawn_materials_ui(sec);

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


                // ── Solver settings ──────────────────────────────────────
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
            if !page_supports_tool(button.page, *tool) {
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

fn page_supports_tool(page: SidebarPage, tool: ViewportTool) -> bool {
    match tool {
        ViewportTool::Selection => true,
        ViewportTool::Assembly => page_supports_part_position(page),
        ViewportTool::LoadDirection => page == SidebarPage::Loads,
    }
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
        *tool = if *tool == ViewportTool::Assembly {
            ViewportTool::Selection
        } else {
            ViewportTool::Assembly
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

// ── post-process result systems ───────────────────────────────────────────────

/// Opens a file dialog for FrontISTR result files (`.res.0.*`), stores the
/// chosen path in a `Local<Option<PathBuf>>` shared with
/// [`result_load_system`].
// ── group creation ────────────────────────────────────────────────────────────

/// Saves the current node selection as one or more mesh-scoped node groups.

/// Updates the result stats text whenever [`FemResultSet`] changes.
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
#[path = "layout_tests.rs"]
mod sidebar_page_tests;
