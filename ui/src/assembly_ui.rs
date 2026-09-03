use crate::assembly::{
    AssemblyEditorState, AssemblyGizmoMode, axis_color, reference_size as assembly_reference_size,
};
use crate::layout::SidebarPage;
use crate::measurement::MeasurementBoxState;
use crate::slider::{SliderId, SliderState, SliderTrack};
use bevy::prelude::*;
use fem_core::{
    ContactCandidateState, FemModel, FemModelVersion, HoverPreviewTargets, ViewportTool,
};
use interaction::HoverResult;
use selection::{Hovered, Selected, SelectionState};

#[path = "assembly_ui_layout.rs"]
mod layout;
pub(crate) use layout::spawn_assembly_ui;

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);
const BUTTON_DISABLED: Color = Color::srgba(0.06, 0.07, 0.08, 0.94);
const ACTIVE_BORDER: Color = Color::srgb(0.57, 0.86, 0.92);

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

#[derive(Debug, Clone, Copy, PartialEq)]
enum AssemblyTransformAction {
    Translate(Vec3),
    Rotate(Vec3),
    Reset,
}

impl AssemblyTransformAction {
    fn for_axis(mode: AssemblyGizmoMode, axis: Vec3) -> Self {
        match mode {
            AssemblyGizmoMode::Move => Self::Translate(axis),
            AssemblyGizmoMode::Rotate => Self::Rotate(axis),
        }
    }

    fn mode(self) -> Option<AssemblyGizmoMode> {
        match self {
            Self::Translate(_) => Some(AssemblyGizmoMode::Move),
            Self::Rotate(_) => Some(AssemblyGizmoMode::Rotate),
            Self::Reset => None,
        }
    }

    fn axis(self) -> Option<Vec3> {
        match self {
            Self::Translate(axis) | Self::Rotate(axis) => Some(axis),
            Self::Reset => None,
        }
    }
}

#[derive(Component)]
pub(crate) struct AssemblyStatusText;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssemblyToolChoice {
    Select,
    Move,
    Rotate,
}

impl AssemblyToolChoice {
    const ALL: [Self; 3] = [Self::Select, Self::Move, Self::Rotate];

    fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Move => "Move",
            Self::Rotate => "Rotate",
        }
    }

    fn gizmo_mode(self) -> Option<AssemblyGizmoMode> {
        match self {
            Self::Select => None,
            Self::Move => Some(AssemblyGizmoMode::Move),
            Self::Rotate => Some(AssemblyGizmoMode::Rotate),
        }
    }

    fn from_state(tool: ViewportTool, mode: AssemblyGizmoMode) -> Self {
        match (tool, mode) {
            (ViewportTool::Assembly, AssemblyGizmoMode::Move) => Self::Move,
            (ViewportTool::Assembly, AssemblyGizmoMode::Rotate) => Self::Rotate,
            _ => Self::Select,
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Select => "Select nodes/faces. Choose Move or Rotate to position a part.",
            Self::Move => "Drag an X/Y/Z arrow or use -/+ below. Shift=fine, Ctrl=snap",
            Self::Rotate => "Drag an RX/RY/RZ ring or use -/+ below. Shift=fine, Ctrl=snap",
        }
    }
}

#[derive(Component)]
pub(crate) struct AssemblyToolButton {
    choice: AssemblyToolChoice,
}

#[derive(Component)]
pub(crate) struct AssemblyToolHint;

#[derive(Component)]
pub(crate) struct AssemblyStepStatusText(AssemblyGizmoMode);

/// None denotes pose actions shared by Move and Rotate, but hidden in Select.
#[derive(Component)]
pub(crate) struct AssemblyNudgeControls(Option<AssemblyGizmoMode>);

fn nudge_controls_display(
    mode: Option<AssemblyGizmoMode>,
    active: AssemblyGizmoMode,
    tool: ViewportTool,
) -> Display {
    if tool == ViewportTool::Assembly && mode.is_none_or(|mode| mode == active) {
        Display::Flex
    } else {
        Display::None
    }
}

pub(crate) fn update_assembly_nudge_visibility(
    state: Res<AssemblyEditorState>,
    tool: Res<ViewportTool>,
    mut controls: Query<(&AssemblyNudgeControls, &mut Node)>,
) {
    for (controls, mut node) in &mut controls {
        let display = nudge_controls_display(controls.0, state.gizmo_mode, *tool);
        if node.display != display {
            node.display = display;
        }
    }
}

fn transform_button_colors(
    action: AssemblyTransformAction,
    interaction: Interaction,
) -> (Color, Color) {
    let Some(axis) = action.axis() else {
        let background = match interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };
        return (background, PANEL_BORDER);
    };

    let axis = axis_color(axis).to_srgba();
    let intensity = match interaction {
        Interaction::None => 0.42,
        Interaction::Hovered => 0.58,
        Interaction::Pressed => 0.76,
    };
    (
        Color::srgba(
            axis.red * intensity,
            axis.green * intensity,
            axis.blue * intensity,
            0.96,
        ),
        Color::srgba(axis.red, axis.green, axis.blue, 0.96),
    )
}

pub(crate) fn rebuild_assembly_parts(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    version: Res<FemModelVersion>,
    mut state: ResMut<AssemblyEditorState>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut last_version: Local<Option<u64>>,
    mut last_signature: Local<Vec<(String, usize, usize)>>,
    container_query: Query<Entity, With<AssemblyPartsContainer>>,
    children_query: Query<&Children>,
) {
    if *last_version == Some(version.value) {
        return;
    }
    *last_version = Some(version.value);

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

pub(crate) fn assembly_tool_button_system(
    mut commands: Commands,
    page: Res<SidebarPage>,
    mut tool: ResMut<ViewportTool>,
    mut state: ResMut<AssemblyEditorState>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut hover: ResMut<HoverResult>,
    mut preview: ResMut<HoverPreviewTargets>,
    mut selection: ResMut<SelectionState>,
    marked_query: Query<Entity, Or<(With<Selected>, With<Hovered>)>>,
    mut buttons: Query<(
        Ref<Interaction>,
        &AssemblyToolButton,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut hints: Query<&mut Text, With<AssemblyToolHint>>,
) {
    let requested = buttons.iter().find_map(|(interaction, button, _, _)| {
        (*interaction == Interaction::Pressed && interaction.is_changed()).then_some(button.choice)
    });
    if matches!(*page, SidebarPage::Model | SidebarPage::Contact)
        && let Some(choice) = requested
        && choice != AssemblyToolChoice::from_state(*tool, state.gizmo_mode)
    {
        *tool = if let Some(mode) = choice.gizmo_mode() {
            state.gizmo_mode = mode;
            ViewportTool::Assembly
        } else {
            ViewportTool::Selection
        };
        state.hovered_part = None;
        state.hovered_axis = None;
        measurement.clear();
        hover.clear();
        preview.targets.clear();
        preview.highlight_targets.clear();
        if *tool == ViewportTool::Assembly {
            selection.clear();
        }
        for entity in &marked_query {
            commands.entity(entity).remove::<Hovered>();
            if *tool == ViewportTool::Assembly {
                commands.entity(entity).remove::<Selected>();
            }
        }
    }

    // Paint after applying the choice so exactly one button is active this frame.
    let active_choice = AssemblyToolChoice::from_state(*tool, state.gizmo_mode);
    for (interaction, button, mut background, mut border) in &mut buttons {
        let active = active_choice == button.choice;
        background.set_if_neq(BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        }));
        border.set_if_neq(BorderColor::all(if active {
            ACTIVE_BORDER
        } else {
            PANEL_BORDER
        }));
    }
    for mut hint in &mut hints {
        hint.set_if_neq(Text::new(active_choice.hint()));
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
    page: Res<SidebarPage>,
    tool: Res<ViewportTool>,
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
    let can_edit = *tool == ViewportTool::Assembly
        && matches!(*page, SidebarPage::Model | SidebarPage::Contact)
        && !state.is_dragging()
        && state
            .selected_part
            .is_some_and(|index| index < model.parts.len());
    for (interaction, button, mut background, mut border) in &mut buttons {
        let enabled = can_edit
            && button
                .action
                .mode()
                .is_none_or(|mode| mode == state.gizmo_mode);
        let mut changed = false;
        if enabled && *interaction == Interaction::Pressed && interaction.is_changed() {
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

        let (button_background, button_border) =
            transform_button_colors(button.action, *interaction);
        background.set_if_neq(BackgroundColor(if enabled {
            button_background
        } else {
            BUTTON_DISABLED
        }));
        border.set_if_neq(BorderColor::all(button_border));
    }
}

pub(crate) fn update_assembly_status_text(
    model: Res<FemModel>,
    version: Res<FemModelVersion>,
    state: Res<AssemblyEditorState>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut last_signature: Local<Option<(u64, Option<usize>, u32, u32)>>,
    mut query: Query<
        (Option<&AssemblyStepStatusText>, &mut Text),
        Or<(With<AssemblyStatusText>, With<AssemblyStepStatusText>)>,
    >,
) {
    let percent = assembly_slider_value(&sliders, SliderId::AssemblyMovePercent, 1.0);
    let degrees = assembly_slider_value(&sliders, SliderId::AssemblyRotationDegrees, 5.0);
    let signature = (
        version.value,
        state.selected_part,
        percent.to_bits(),
        degrees.to_bits(),
    );
    if *last_signature == Some(signature) {
        return;
    }
    *last_signature = Some(signature);

    let selected = state
        .selected_part
        .and_then(|index| model.parts.get(index).map(|part| (index, part)));
    let Some((part_index, part)) = selected else {
        for (_, mut text) in &mut query {
            text.set_if_neq(Text::new("No part selected"));
        }
        return;
    };
    let center = model.part_centroid(part_index).unwrap_or(Vec3::ZERO);
    let move_step = assembly_reference_size(&model, part_index) * percent / 100.0;
    for (step, mut text) in &mut query {
        let content = match step.map(|step| step.0) {
            None => format!(
                "Selected: {}\nCenter: ({:.4}, {:.4}, {:.4})",
                part.name, center.x, center.y, center.z,
            ),
            Some(AssemblyGizmoMode::Move) => format!("Move step: {move_step:.6} model units"),
            Some(AssemblyGizmoMode::Rotate) => format!("Rotate step: {degrees:.4} deg"),
        };
        text.set_if_neq(Text::new(content));
    }
}

#[cfg(test)]
#[path = "assembly_ui_tests.rs"]
mod tests;
