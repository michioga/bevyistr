//! Shared viewport selection controls, growth modes, previews, and groups.

use crate::layout::{RenderModeButton, ScrollableList, SidebarPage};
use crate::slider::{SliderId, SliderState, SliderTrack};
use bevy::prelude::*;
use bevy::ui::ScrollPosition;
use fem_core::{
    FemEntityId, FemEntityRef, FemModel, FemModelVersion, SelectionFilter, SelectionLevel,
};
use interaction::HoverResult;
use selection::{Hovered, Selectable, Selected, SelectionOperation, SelectionState};
use std::collections::{BTreeMap, BTreeSet};

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);
const ACTIVE_BORDER: Color = Color::srgb(0.57, 0.86, 0.92);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const COPLANAR_TOLERANCE_DEG: f32 = 0.5;
pub(crate) const DEFAULT_SMOOTH_ANGLE_DEG: f32 = 15.0;

pub(crate) const SELECTION_GUIDE_TEXT: &str = "Click / drag       Replace selection\n\
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
    pub(crate) level: SelectionLevel,
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
pub(crate) struct MakeNodeGroupButton;

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
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Single => "Single",
            Self::Coplanar => "Coplanar",
            Self::Smooth => "Smooth",
        }
    }
}

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
    pub(crate) mode: SurfaceSelectionMode,
}

#[derive(Component)]
pub(crate) struct SurfaceSelectionHint;

#[derive(Component)]
pub(crate) struct SurfaceAngleControls;

#[derive(Component)]
pub(crate) struct SurfaceSelectionControls;

#[derive(Component)]
pub(crate) struct SurfaceSelectionUnavailableHint;

#[derive(Component)]
pub(crate) struct SelectionStatsText;

#[derive(Component)]
pub(crate) struct SelectionInfoText;

/// Which kind of [`fem_core::FemMesh`] set a [`SetButton`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetKind {
    Node,
    Element,
    Surface,
}

/// References one mesh-scoped set so clicking its row can select all members.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SetButton {
    mesh_index: usize,
    kind: SetKind,
    set_index: usize,
}

/// Dynamic list rebuilt from the node, element, and surface sets in the model.
#[derive(Component)]
pub(crate) struct SetsListContainer;

pub(crate) fn spawn_model_selection_ui(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Text::new("Filter: Element   Selected: 0   Hover: none"),
        TextFont {
            font_size: FontSize::Px(11.5),
            ..default()
        },
        TextColor(TEXT_MAIN),
        SelectionStatsText,
    ));
    parent.spawn((
        Text::new("Selected: 0  |  Hover: -"),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(Color::srgba(0.50, 0.78, 0.95, 0.90)),
        SelectionInfoText,
    ));

    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(6.0),
            ..default()
        },))
        .with_children(|row| {
            selection_action_button(
                row,
                "Make Node Group",
                MakeNodeGroupButton,
                "MakeNodeGroupButton",
            );
            selection_action_button(
                row,
                "Make Element Group",
                MakeElementGroupButton,
                "MakeElementGroupButton",
            );
        });
    selection_hint(
        parent,
        "Saves selection as NGRP/EGRP for use in BCs and sections",
    );
}

pub(crate) fn spawn_model_sets_ui(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
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
    selection_hint(
        parent,
        "Click a set to select its members   Scroll to see more",
    );
}

fn selection_action_button(
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

fn selection_hint(parent: &mut ChildSpawnerCommands, text: &'static str) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(Color::srgba(0.45, 0.54, 0.60, 0.80)),
    ));
}

pub(crate) struct SelectionPageContext {
    pub(crate) label: &'static str,

    pub(crate) levels: &'static [SelectionLevel],

    pub(crate) preferred: SelectionLevel,
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

pub(crate) fn selection_context_for_page(page: SidebarPage) -> SelectionPageContext {
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

pub(crate) fn selection_operation_hint(operation: SelectionOperation) -> (&'static str, Color) {
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

pub(crate) const fn supports_surface_growth(level: SelectionLevel) -> bool {
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

pub(crate) fn surface_selection_hint(
    level: SelectionLevel,
    mode: SurfaceSelectionMode,
) -> &'static str {
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

pub(crate) fn rebuild_sets_list(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    version: Res<FemModelVersion>,
    mut last_version: Local<Option<u64>>,
    container_query: Query<Entity, With<SetsListContainer>>,
    children_query: Query<&Children>,
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

fn set_list_button(parent: &mut ChildSpawnerCommands, set_button: SetButton, label: &str) {
    parent
        .spawn((
            Button,
            Node {
                width: percent(100.0),
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

/// Selects every member of a clicked mesh set using the same modifier
/// semantics as viewport selection.
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
        if *interaction == Interaction::Pressed
            && interaction.is_changed()
            && let Some(model) = model.as_deref()
            && let Some(mesh) = model.meshes.get(set_button.mesh_index)
        {
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
                let targets: Vec<FemEntityRef> = local_targets
                    .into_iter()
                    .map(|target| FemEntityRef::new(set_button.mesh_index, target))
                    .collect();
                let ctrl = keyboard.pressed(KeyCode::ControlLeft)
                    || keyboard.pressed(KeyCode::ControlRight);
                let shift =
                    keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
                let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
                let operation = SelectionOperation::from_modifiers(ctrl, shift, alt);

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

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };
        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

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

fn selection_level_label(level: SelectionLevel) -> &'static str {
    match level {
        SelectionLevel::Node => "Node",
        SelectionLevel::Edge => "Edge",
        SelectionLevel::Face => "Face",
        SelectionLevel::Element => "Element",
    }
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

fn segment_style(is_first: bool, is_last: bool) -> (BorderRadius, UiRect) {
    let radius = match (is_first, is_last) {
        (true, true) => BorderRadius::all(px(4.0)),
        (true, false) => BorderRadius::left(px(4.0)),
        (false, true) => BorderRadius::right(px(4.0)),
        (false, false) => BorderRadius::ZERO,
    };
    let border = UiRect {
        left: px(1.0),
        right: px(if is_last { 1.0 } else { 0.0 }),
        top: px(1.0),
        bottom: px(1.0),
    };
    (radius, border)
}

fn selected_nodes_by_mesh(selection: &SelectionState) -> BTreeMap<usize, Vec<fem_core::NodeId>> {
    let mut grouped = BTreeMap::<usize, BTreeSet<fem_core::NodeId>>::new();
    for target in &selection.targets {
        if let FemEntityId::Node(id) = target.entity {
            grouped.entry(target.mesh_index).or_default().insert(id);
        }
    }
    grouped
        .into_iter()
        .map(|(mesh_index, nodes)| (mesh_index, nodes.into_iter().collect()))
        .collect()
}
