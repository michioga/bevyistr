use crate::assembly::{
    AssemblyEditorState, AssemblyGizmoMode, axis_color, reference_size as assembly_reference_size,
};
use crate::assembly_clearance::spawn_assembly_clearance_ui;
use crate::layout::ScrollableList;
use crate::measurement::MeasurementBoxState;
use crate::slider::{SliderConfig, SliderId, SliderState, SliderTrack, spawn_slider};
use bevy::prelude::*;
use bevy::ui::ScrollPosition;
use fem_core::{ContactCandidateState, FemModel, FemModelVersion, ViewportTool};
use interaction::HoverResult;
use selection::{Selected, SelectionState};

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);
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

#[derive(Debug, Clone, Copy)]
enum AssemblyTransformAction {
    Translate(Vec3),
    Rotate(Vec3),
    Reset,
}

impl AssemblyTransformAction {
    fn axis(self) -> Option<Vec3> {
        match self {
            Self::Translate(axis) | Self::Rotate(axis) => Some(axis),
            Self::Reset => None,
        }
    }
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

pub(crate) fn spawn_assembly_ui(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
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
            TextFont {
                font_size: FontSize::Px(11.5),
                ..default()
            },
            TextColor(TEXT_MAIN),
            AssemblyModeButtonLabel,
        ));

    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            ..default()
        },))
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
        parent,
        "Move: drag X/Y/Z arrow   Rotate: drag RX/RY/RZ ring",
    );
    hint_text(parent, "Turn Edit OFF to return to node/face selection");

    parent.spawn((
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
    parent.spawn((
        Text::new("No part selected"),
        TextFont {
            font_size: FontSize::Px(10.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
        AssemblyStatusText,
    ));

    spawn_assembly_clearance_ui(parent);

    spawn_slider(
        parent,
        SliderConfig {
            width: 272.0,
            min: 0.1,
            max: 10.0,
            value: 1.0,
            label: "Move step (% of part size)",
            id: SliderId::AssemblyMovePercent,
        },
    );
    assembly_action_row(
        parent,
        [
            ("-X", AssemblyTransformAction::Translate(-Vec3::X)),
            ("-Y", AssemblyTransformAction::Translate(-Vec3::Y)),
            ("-Z", AssemblyTransformAction::Translate(-Vec3::Z)),
        ],
    );
    assembly_action_row(
        parent,
        [
            ("+X", AssemblyTransformAction::Translate(Vec3::X)),
            ("+Y", AssemblyTransformAction::Translate(Vec3::Y)),
            ("+Z", AssemblyTransformAction::Translate(Vec3::Z)),
        ],
    );

    spawn_slider(
        parent,
        SliderConfig {
            width: 272.0,
            min: 1.0,
            max: 45.0,
            value: 5.0,
            label: "Rotate step (deg)",
            id: SliderId::AssemblyRotationDegrees,
        },
    );
    assembly_action_row(
        parent,
        [
            ("-RX", AssemblyTransformAction::Rotate(-Vec3::X)),
            ("-RY", AssemblyTransformAction::Rotate(-Vec3::Y)),
            ("-RZ", AssemblyTransformAction::Rotate(-Vec3::Z)),
        ],
    );
    assembly_action_row(
        parent,
        [
            ("+RX", AssemblyTransformAction::Rotate(Vec3::X)),
            ("+RY", AssemblyTransformAction::Rotate(Vec3::Y)),
            ("+RZ", AssemblyTransformAction::Rotate(Vec3::Z)),
        ],
    );

    action_button(
        parent,
        "Reset selected part pose",
        AssemblyTransformButton {
            action: AssemblyTransformAction::Reset,
        },
        "AssemblyResetPoseButton",
        BUTTON_NORMAL,
        PANEL_BORDER,
    );
    hint_text(
        parent,
        "Real mesh coordinates are updated. Contact candidates are cleared after movement; run Detect again",
    );
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
                let (background, border) = transform_button_colors(action, Interaction::None);
                action_button(
                    row,
                    label,
                    AssemblyTransformButton { action },
                    "AssemblyTransformButton",
                    background,
                    border,
                );
            }
        });
}

fn action_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &'static str,
    marker: M,
    name: &'static str,
    background: Color,
    border: Color,
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
            BackgroundColor(background),
            BorderColor::all(border),
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
    let radius = 5.0;
    let border = UiRect {
        top: px(1.0),
        bottom: px(1.0),
        left: if is_first { px(1.0) } else { px(0.0) },
        right: px(1.0),
    };
    let (top_left, bottom_left) = if is_first {
        (radius, radius)
    } else {
        (0.0, 0.0)
    };
    let (top_right, bottom_right) = if is_last {
        (radius, radius)
    } else {
        (0.0, 0.0)
    };
    (
        BorderRadius::new(
            px(top_left),
            px(top_right),
            px(bottom_right),
            px(bottom_left),
        ),
        border,
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

        let (button_background, button_border) =
            transform_button_colors(button.action, *interaction);
        *background = BackgroundColor(button_background);
        *border = BorderColor::all(button_border);
    }
}

pub(crate) fn update_assembly_status_text(
    model: Res<FemModel>,
    version: Res<FemModelVersion>,
    state: Res<AssemblyEditorState>,
    tool: Res<ViewportTool>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut last_signature: Local<Option<(u64, Option<usize>, ViewportTool, AssemblyGizmoMode, u32)>>,
    mut query: Query<&mut Text, With<AssemblyStatusText>>,
) {
    let percent = assembly_slider_value(&sliders, SliderId::AssemblyMovePercent, 1.0);
    let signature = (
        version.value,
        state.selected_part,
        *tool,
        state.gizmo_mode,
        percent.to_bits(),
    );
    if *last_signature == Some(signature) {
        return;
    }
    *last_signature = Some(signature);

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
