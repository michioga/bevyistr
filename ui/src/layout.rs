use crate::assembly_ui::spawn_assembly_ui;
use crate::bc_loads_ui::{BoundaryLoadsListContainer, spawn_boundary_load_editor};
use crate::contact_ui::{
    spawn_contact_definition_ui, spawn_contact_detection_ui, spawn_contact_review_ui,
};
use crate::materials_ui::spawn_materials_ui;
use crate::measurement::MeasurementBoxState;
use crate::mpc_ui::spawn_mpc_ui;
use crate::project_io::spawn_model_file_ui;
use crate::results_ui::{
    OpenResultButton, PlaybackEndButton, PlaybackPlayPauseButton, PlaybackPlayPauseLabel,
    PlaybackRewindButton, ResultSliderSection, ResultStatsText,
};
use crate::selection_ui::{
    DEFAULT_SMOOTH_ANGLE_DEG, SELECTION_GUIDE_TEXT, SelectionContextText, SelectionGuidePanel,
    SelectionGuideToggle, SelectionLevelButton, SelectionOperationHint, SelectionToolbar,
    SurfaceAngleControls, SurfaceSelectionControls, SurfaceSelectionHint, SurfaceSelectionMode,
    SurfaceSelectionModeButton, SurfaceSelectionUnavailableHint, spawn_model_selection_ui,
    spawn_model_sets_ui,
};
use crate::slider::{SliderConfig, SliderId, spawn_slider};
use crate::solve_ui::spawn_solve_ui;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::ScrollPosition;
use fem_core::{SelectionLevel, UiPointerState, ViewportTool};
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

/// Marks any scrollable list container so [`handle_scrollable_list_wheel`]
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
            panel
                .spawn((
                    Node {
                        padding: UiRect::axes(px(12.0), px(8.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.08, 0.11, 0.14, 0.98)),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        Text::new("bevyistr"),
                        TextFont {
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
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
            panel
                .spawn((
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
                    sidebar_page_group(
                        panel,
                        SidebarPageContent::page(SidebarPage::Model),
                        "ModelFilePage",
                        |panel| {
                            // ── § File ──────────────────────────────────────────────────
                            section(panel, "FILE", |sec| {
                                spawn_model_file_ui(sec);
                            });
                            divider(panel);
                        },
                    ); // end ModelFilePage

                    sidebar_page_group(
                        panel,
                        SidebarPageContent::part_position(),
                        "PartPositionPage",
                        |panel| {
                            // ── § Assembly ──────────────────────────────────────────────
                            section(panel, "PART POSITION", |sec| {
                                spawn_assembly_ui(sec);
                            });
                            divider(panel);
                        },
                    ); // end PartPositionPage

                    sidebar_page_group(
                        panel,
                        SidebarPageContent::page(SidebarPage::Model),
                        "ModelSelectionPage",
                        |panel| {
                            // ── § Selection ─────────────────────────────────────────────
                            section(panel, "SELECTION", |sec| {
                                spawn_model_selection_ui(sec);
                            });
                            divider(panel);
                        },
                    ); // end ModelSelectionPage

                    sidebar_page_group(
                        panel,
                        SidebarPageContent::page(SidebarPage::Model),
                        "ModelSetsPage",
                        |panel| {
                            // ── § Sets / Groups ──────────────────────────────────────────
                            section(panel, "SETS", |sec| {
                                spawn_model_sets_ui(sec);
                            });
                            divider(panel);
                        },
                    ); // end ModelSetsPage

                    sidebar_page_group(
                        panel,
                        SidebarPageContent::page(SidebarPage::Contact),
                        "ContactPage",
                        |panel| {
                            // ── § Contact definition ─────────────────────────────────────
                            section(panel, "CONTACT DEFINITION", spawn_contact_definition_ui);
                            divider(panel);

                            // ── § Contact detection ──────────────────────────────────────
                            section(panel, "CONTACT DETECTION", spawn_contact_detection_ui);
                            divider(panel);

                            // ── § Contact review ─────────────────────────────────────────
                            section(panel, "CONTACT REVIEW", spawn_contact_review_ui);
                            divider(panel);

                            // ── § MPC / rigid spider ─────────────────────────────────────
                            section(panel, "MPC / RIGID SPIDER", spawn_mpc_ui);
                            divider(panel);
                        },
                    ); // end ContactPage

                    sidebar_page_group(
                        panel,
                        SidebarPageContent::analysis(),
                        "AnalysisPages",
                        |panel| {
                            // ── § Analysis Setup (boundary conditions / loads / materials) ──
                            section(panel, "ANALYSIS SETUP", |sec| {
                                // Export row: Open Setup + Export to FrontISTR on the same row
                                spawn_solve_ui(sec);
                                spawn_boundary_load_editor(sec);

                                spawn_materials_ui(sec);

                                sec.spawn((
                                    Text::new("Boundary conditions & loads:"),
                                    TextFont {
                                        font_size: FontSize::Px(9.5),
                                        ..default()
                                    },
                                    TextColor(Color::srgba(0.44, 0.60, 0.72, 0.90)),
                                    Node {
                                        margin: UiRect::top(px(4.0)),
                                        ..default()
                                    },
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
                        },
                    ); // end AnalysisPages

                    // ── POST-PROCESS group ──────────────────────────────────────────
                    sidebar_page_group(
                        panel,
                        SidebarPageContent::page(SidebarPage::Results),
                        "ResultsPage",
                        |panel| {
                            // ── § Post-process ───────────────────────────────────────────
                            section(panel, "POST-PROCESS", |sec| {
                                sec.spawn((Node {
                                    flex_direction: FlexDirection::Row,
                                    ..default()
                                },))
                                    .with_children(|row| {
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
                                        ))
                                        .with_child((
                                            Text::new("Open Result  (.res / .vtu)"),
                                            TextFont {
                                                font_size: FontSize::Px(12.0),
                                                ..default()
                                            },
                                            TextColor(TEXT_MAIN),
                                        ));
                                    });
                                sec.spawn((
                                    Text::new("Result: none loaded"),
                                    TextFont {
                                        font_size: FontSize::Px(11.5),
                                        ..default()
                                    },
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
                                ))
                                .with_children(|sliders| {
                                    spawn_slider(
                                        sliders,
                                        SliderConfig {
                                            width: 270.0,
                                            min: 0.0,
                                            max: 0.0,
                                            value: 0.0,
                                            label: "Step",
                                            id: SliderId::ResultStep,
                                        },
                                    );
                                    spawn_slider(
                                        sliders,
                                        SliderConfig {
                                            width: 270.0,
                                            min: 0.0,
                                            max: 20.0,
                                            value: 1.0,
                                            label: "Deform scale",
                                            id: SliderId::DeformScale,
                                        },
                                    );
                                    hint_text(sliders, "Left / Right: step through results");

                                    // ── Animation playback controls ──────────────────────
                                    sliders
                                        .spawn((Node {
                                            flex_direction: FlexDirection::Row,
                                            column_gap: px(4.0),
                                            margin: UiRect::top(px(6.0)),
                                            ..default()
                                        },))
                                        .with_children(|row| {
                                            // ◀◀
                                            row.spawn((
                                                Button,
                                                Node {
                                                    width: px(36.0),
                                                    height: px(28.0),
                                                    justify_content: JustifyContent::Center,
                                                    align_items: AlignItems::Center,
                                                    border: UiRect::all(px(1.0)),
                                                    border_radius: BorderRadius::all(px(5.0)),
                                                    ..default()
                                                },
                                                BackgroundColor(BUTTON_NORMAL),
                                                BorderColor::all(PANEL_BORDER),
                                                PlaybackRewindButton,
                                            ))
                                            .with_child((
                                                Text::new("|<"),
                                                TextFont {
                                                    font_size: FontSize::Px(10.0),
                                                    ..default()
                                                },
                                                TextColor(TEXT_MAIN),
                                            ));

                                            // ▶ / ‖
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
                                                PlaybackPlayPauseButton,
                                            ))
                                            .with_child((
                                                Text::new("Play"),
                                                TextFont {
                                                    font_size: FontSize::Px(11.0),
                                                    ..default()
                                                },
                                                TextColor(TEXT_MAIN),
                                                PlaybackPlayPauseLabel,
                                            ));

                                            // ▶▶
                                            row.spawn((
                                                Button,
                                                Node {
                                                    width: px(36.0),
                                                    height: px(28.0),
                                                    justify_content: JustifyContent::Center,
                                                    align_items: AlignItems::Center,
                                                    border: UiRect::all(px(1.0)),
                                                    border_radius: BorderRadius::all(px(5.0)),
                                                    ..default()
                                                },
                                                BackgroundColor(BUTTON_NORMAL),
                                                BorderColor::all(PANEL_BORDER),
                                                PlaybackEndButton,
                                            ))
                                            .with_child((
                                                Text::new(">|"),
                                                TextFont {
                                                    font_size: FontSize::Px(10.0),
                                                    ..default()
                                                },
                                                TextColor(TEXT_MAIN),
                                            ));
                                        });

                                    spawn_slider(
                                        sliders,
                                        SliderConfig {
                                            width: 270.0,
                                            min: 0.5,
                                            max: 10.0,
                                            value: 2.0,
                                            label: "Speed (steps/sec)",
                                            id: SliderId::PlaybackSpeed,
                                        },
                                    );
                                });
                            });
                        },
                    ); // end ResultsPage
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

#[cfg(test)]
#[path = "layout_tests.rs"]
mod sidebar_page_tests;
