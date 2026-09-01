//! Exact engineering-value editor for boundary conditions and loads.
//!
//! Viewport picking remains the fast way to choose geometry and a principal
//! direction.  The values in this module are the authoritative draft: they
//! expose all six structural degrees of freedom and are copied to
//! [`fem_core::AnalysisSetup`] only by an explicit Apply action.

use crate::bc_loads_ui::{ActiveLoadEditor, SelectedDloadKind, SelectedLoadDirection};
use crate::layout::UiInputCapture;
use crate::measurement::{editable_value, format_measurement, parse_measurement};
use crate::slider::{SliderId, SliderState, SliderTrack};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, TextCursorStyle};
use bevy::ui_widgets::SelectAllOnFocus;
use fem_core::{FemEntityId, FemModel, UiKeyboardState};
use selection::SelectionState;
use std::collections::{BTreeMap, BTreeSet};

const INPUT_BG: Color = Color::srgba(0.075, 0.09, 0.10, 0.98);
const INPUT_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const INPUT_FOCUS_BORDER: Color = Color::srgb(0.57, 0.86, 0.92);
const INPUT_ERROR_BORDER: Color = Color::srgb(0.98, 0.36, 0.30);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);

/// Values being authored in the BC / Loads page.  These are intentionally
/// separate from `AnalysisSetup`: selecting geometry or typing is reversible
/// preview work, while Apply is the transaction boundary recorded by undo.
#[derive(Resource, Debug, Clone, PartialEq)]
pub(crate) struct BoundaryLoadEditorState {
    pub constraint_enabled: [bool; 6],
    pub constraint_values: [f32; 6],
    pub nodal_components: [f32; 6],
    pub pressure: f32,
    pub gravity_acceleration: f32,
    pub gravity_direction: Vec3,
    pub constraint_rotation_mode: RotationalInputMode,
    pub load_moment_mode: RotationalInputMode,
    pub rotation_center: Option<fem_core::FemEntityRef>,
    pub rotation_center_feedback: Option<String>,
    pub error: Option<(EngineeringField, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RotationalInputMode {
    #[default]
    DirectDof,
    AboutCenter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RotationalInputKind {
    Constraint,
    Load,
}

/// Last values observed from the coarse sliders. Bevy change detection also
/// fires when a slider's `dragging` flag changes, so value comparison keeps an
/// unrelated mouse release from overwriting an exact text entry.
#[derive(Resource, Debug, Default)]
pub(crate) struct QuickLoadControlState {
    load_value: Option<f32>,
    dload_value: Option<f32>,
}

impl Default for BoundaryLoadEditorState {
    fn default() -> Self {
        Self {
            constraint_enabled: [false; 6],
            constraint_values: [0.0; 6],
            nodal_components: [0.0; 6],
            pressure: 1.0,
            gravity_acceleration: 1.0,
            gravity_direction: Vec3::NEG_Y,
            constraint_rotation_mode: RotationalInputMode::DirectDof,
            load_moment_mode: RotationalInputMode::DirectDof,
            rotation_center: None,
            rotation_center_feedback: None,
            error: None,
        }
    }
}

impl BoundaryLoadEditorState {
    pub(crate) fn set_constraint_preset(&mut self, dof_start: u8, dof_end: u8) {
        self.constraint_enabled = [false; 6];
        for dof in dof_start.max(1)..=dof_end.min(6) {
            let index = usize::from(dof - 1);
            self.constraint_enabled[index] = true;
            self.constraint_values[index] = 0.0;
        }
    }

    pub(crate) fn set_axis_force(&mut self, dof: u8, sign: f32, magnitude: f32) {
        if !(1..=3).contains(&dof) || !magnitude.is_finite() {
            return;
        }
        self.nodal_components[..3].fill(0.0);
        self.nodal_components[usize::from(dof - 1)] = magnitude.abs() * sign.signum();
    }

    pub(crate) fn single_axis_force(&self) -> Option<((u8, f32), f32)> {
        let nonzero: Vec<_> = self.nodal_components[..3]
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, value)| value.abs() > f32::EPSILON)
            .collect();
        let [(index, value)] = nonzero.as_slice() else {
            return None;
        };
        Some((((*index + 1) as u8, value.signum()), value.abs()))
    }

    pub(crate) fn translational_force(&self) -> Vec3 {
        Vec3::from_array([
            self.nodal_components[0],
            self.nodal_components[1],
            self.nodal_components[2],
        ])
    }

    pub(crate) fn rotational_moment(&self) -> Vec3 {
        Vec3::from_array([
            self.nodal_components[3],
            self.nodal_components[4],
            self.nodal_components[5],
        ])
    }

    pub(crate) fn normalized_gravity_direction(&self) -> Option<Vec3> {
        self.gravity_direction.try_normalize()
    }

    pub(crate) fn resolved_rotation_center(&self) -> Option<fem_core::RotationCenter> {
        let center = self.rotation_center?;
        let FemEntityId::Node(node) = center.entity else {
            return None;
        };
        Some(fem_core::RotationCenter::from_node(center.mesh_index, node))
    }

    pub(crate) fn constraint_solver_component(
        &self,
        index: usize,
    ) -> Option<(u8, Option<fem_core::RotationCenter>)> {
        match index {
            0..=2 => Some((index as u8 + 1, None)),
            3..=5 if self.constraint_rotation_mode == RotationalInputMode::DirectDof => {
                Some((index as u8 + 1, None))
            }
            3..=5 => self
                .resolved_rotation_center()
                .map(|center| (index as u8 - 2, Some(center))),
            _ => None,
        }
    }

    pub(crate) fn load_solver_component(
        &self,
        index: usize,
    ) -> Option<(u8, Option<fem_core::RotationCenter>)> {
        match index {
            0..=2 => Some((index as u8 + 1, None)),
            3..=5 if self.load_moment_mode == RotationalInputMode::DirectDof => {
                Some((index as u8 + 1, None))
            }
            3..=5 => self
                .resolved_rotation_center()
                .map(|center| (index as u8 - 2, Some(center))),
            _ => None,
        }
    }

    fn value(&self, field: EngineeringField) -> f32 {
        match field {
            EngineeringField::Constraint(dof) => self.constraint_values[usize::from(dof - 1)],
            EngineeringField::Nodal(dof) => self.nodal_components[usize::from(dof - 1)],
            EngineeringField::Pressure => self.pressure,
            EngineeringField::GravityAcceleration => self.gravity_acceleration,
            EngineeringField::GravityDirection(axis) => self.gravity_direction[usize::from(axis)],
        }
    }

    fn set_value(&mut self, field: EngineeringField, value: f32) {
        match field {
            EngineeringField::Constraint(dof) => {
                self.constraint_values[usize::from(dof - 1)] = value
            }
            EngineeringField::Nodal(dof) => self.nodal_components[usize::from(dof - 1)] = value,
            EngineeringField::Pressure => self.pressure = value,
            EngineeringField::GravityAcceleration => self.gravity_acceleration = value,
            EngineeringField::GravityDirection(axis) => {
                self.gravity_direction[usize::from(axis)] = value
            }
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EngineeringField {
    Constraint(u8),
    Nodal(u8),
    Pressure,
    GravityAcceleration,
    GravityDirection(u8),
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct EngineeringValueInput(pub EngineeringField);

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ConstraintDofToggle(pub u8);

#[derive(Component)]
pub(crate) struct ApplyConstraintButton;

#[derive(Component)]
pub(crate) struct ApplyConstraintLabel;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct RotationalInputModeButton {
    pub kind: RotationalInputKind,
    pub mode: RotationalInputMode,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) enum RotationCenterButton {
    Capture,
    Clear,
}

#[derive(Component)]
pub(crate) struct RotationCenterStatus;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct DloadExactFieldGroup(pub SelectedDloadKind);

#[derive(Component)]
pub(crate) struct EngineeringInputStatus;

pub(crate) fn spawn_constraint_exact_editor(parent: &mut ChildSpawnerCommands) {
    section_caption(parent, "Prescribed displacement / rotation (exact)");
    component_grid(
        parent,
        &[
            ("Ux", EngineeringField::Constraint(1)),
            ("Uy", EngineeringField::Constraint(2)),
            ("Uz", EngineeringField::Constraint(3)),
            ("Rx", EngineeringField::Constraint(4)),
            ("Ry", EngineeringField::Constraint(5)),
            ("Rz", EngineeringField::Constraint(6)),
        ],
        true,
    );
    spawn_rotational_input_controls(parent, RotationalInputKind::Constraint);
    apply_button(
        parent,
        "Apply Constraints",
        (ApplyConstraintButton,),
        (ApplyConstraintLabel,),
        "ApplyConstraintButton",
    );
}

pub(crate) fn spawn_nodal_exact_editor(parent: &mut ChildSpawnerCommands) {
    section_caption(parent, "Nodal force / moment components (exact)");
    component_grid(
        parent,
        &[
            ("Fx", EngineeringField::Nodal(1)),
            ("Fy", EngineeringField::Nodal(2)),
            ("Fz", EngineeringField::Nodal(3)),
            ("Mx", EngineeringField::Nodal(4)),
            ("My", EngineeringField::Nodal(5)),
            ("Mz", EngineeringField::Nodal(6)),
        ],
        false,
    );
    spawn_rotational_input_controls(parent, RotationalInputKind::Load);
}

fn spawn_rotational_input_controls(parent: &mut ChildSpawnerCommands, kind: RotationalInputKind) {
    let caption = match kind {
        RotationalInputKind::Constraint => "Rx/Ry/Rz interpretation",
        RotationalInputKind::Load => "Mx/My/Mz interpretation",
    };
    section_caption(parent, caption);
    parent
        .spawn((Node {
            width: percent(100.0),
            flex_direction: FlexDirection::Row,
            column_gap: px(4.0),
            ..default()
        },))
        .with_children(|row| {
            for (label, mode) in [
                ("Direct DOF", RotationalInputMode::DirectDof),
                ("About center", RotationalInputMode::AboutCenter),
            ] {
                row.spawn((
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
                    BorderColor::all(INPUT_BORDER),
                    RotationalInputModeButton { kind, mode },
                ))
                .with_child((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));
            }
        });
    parent
        .spawn((Node {
            width: percent(100.0),
            flex_direction: FlexDirection::Row,
            column_gap: px(4.0),
            ..default()
        },))
        .with_children(|row| {
            for (label, action, grow) in [
                (
                    "Set center from selected node",
                    RotationCenterButton::Capture,
                    1.0,
                ),
                ("Clear", RotationCenterButton::Clear, 0.0),
            ] {
                row.spawn((
                    Button,
                    Node {
                        flex_grow: grow,
                        min_width: if grow > 0.0 { px(0.0) } else { px(52.0) },
                        height: px(22.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(4.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(INPUT_BORDER),
                    action,
                ))
                .with_child((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(8.5),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));
            }
        });
    parent.spawn((
        Text::new("Center: not set"),
        TextFont {
            font_size: FontSize::Px(8.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
        RotationCenterStatus,
    ));
}

pub(crate) fn spawn_dload_exact_editor(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                ..default()
            },
            DloadExactFieldGroup(SelectedDloadKind::Pressure),
            Name::new("Exact pressure fields"),
        ))
        .with_children(|group| {
            section_caption(group, "Pressure magnitude (exact)");
            wide_value_row(group, "P", EngineeringField::Pressure);
        });

    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                ..default()
            },
            Visibility::Hidden,
            DloadExactFieldGroup(SelectedDloadKind::Gravity),
            Name::new("Exact gravity fields"),
        ))
        .with_children(|group| {
            section_caption(group, "Gravity acceleration and direction (exact)");
            wide_value_row(group, "g", EngineeringField::GravityAcceleration);
            component_grid(
                group,
                &[
                    ("X", EngineeringField::GravityDirection(0)),
                    ("Y", EngineeringField::GravityDirection(1)),
                    ("Z", EngineeringField::GravityDirection(2)),
                ],
                false,
            );
        });
}

pub(crate) fn spawn_engineering_input_status(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Text::new("Values are provisional until Apply"),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(TEXT_MUTED),
        EngineeringInputStatus,
    ));
}

fn section_caption(parent: &mut ChildSpawnerCommands, text: &'static str) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(TEXT_MUTED),
    ));
}

fn component_grid(
    parent: &mut ChildSpawnerCommands,
    fields: &[(&'static str, EngineeringField)],
    constraint_toggles: bool,
) {
    for row_fields in fields.chunks(3) {
        parent
            .spawn((Node {
                width: percent(100.0),
                flex_direction: FlexDirection::Row,
                column_gap: px(4.0),
                ..default()
            },))
            .with_children(|row| {
                for &(label, field) in row_fields {
                    if label.is_empty() {
                        continue;
                    }
                    compact_value_field(row, label, field, constraint_toggles);
                }
            });
    }
}

fn compact_value_field(
    parent: &mut ChildSpawnerCommands,
    label: &'static str,
    field: EngineeringField,
    constraint_toggle: bool,
) {
    parent
        .spawn((Node {
            flex_grow: 1.0,
            min_width: px(82.0),
            height: px(25.0),
            flex_direction: FlexDirection::Row,
            column_gap: px(2.0),
            ..default()
        },))
        .with_children(|cell| {
            if constraint_toggle {
                let EngineeringField::Constraint(dof) = field else {
                    return;
                };
                cell.spawn((
                    Button,
                    Node {
                        width: px(28.0),
                        height: px(25.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(3.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(INPUT_BORDER),
                    ConstraintDofToggle(dof),
                ))
                .with_child((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));
            } else {
                cell.spawn((
                    Text::new(label),
                    Node {
                        width: px(24.0),
                        height: px(25.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));
            }
            numeric_input(cell, field);
        });
}

fn wide_value_row(parent: &mut ChildSpawnerCommands, label: &'static str, field: EngineeringField) {
    parent
        .spawn((Node {
            width: percent(100.0),
            height: px(25.0),
            flex_direction: FlexDirection::Row,
            column_gap: px(4.0),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                Node {
                    width: px(28.0),
                    height: px(25.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));
            numeric_input(row, field);
        });
}

fn numeric_input(parent: &mut ChildSpawnerCommands, field: EngineeringField) {
    parent.spawn((
        Node {
            flex_grow: 1.0,
            min_width: px(46.0),
            height: px(25.0),
            padding: UiRect::axes(px(4.0), px(2.0)),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(3.0)),
            ..default()
        },
        EditableText {
            visible_width: Some(10.0),
            max_characters: Some(32),
            allow_newlines: false,
            ..EditableText::new("0")
        },
        EditableTextFilter::new(|character| {
            character.is_ascii_digit() || matches!(character, '+' | '-' | '.' | 'e' | 'E')
        }),
        TextLayout {
            justify: Justify::End,
            ..TextLayout::no_wrap()
        },
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(TEXT_MAIN),
        TextCursorStyle {
            color: INPUT_FOCUS_BORDER,
            selected_text_color: Some(Color::srgb(0.02, 0.04, 0.05)),
            ..default()
        },
        SelectAllOnFocus,
        BackgroundColor(INPUT_BG),
        BorderColor::all(INPUT_BORDER),
        UiInputCapture,
        EngineeringValueInput(field),
    ));
}

fn apply_button(
    parent: &mut ChildSpawnerCommands,
    label: &'static str,
    marker: impl Bundle,
    label_marker: impl Bundle,
    name: &'static str,
) {
    parent
        .spawn((
            Button,
            Node {
                width: percent(100.0),
                height: px(24.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(INPUT_BORDER),
            marker,
            Name::new(name),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(TEXT_MAIN),
            label_marker,
        ));
}

/// Keeps text fields and the typed draft synchronized. Invalid text never
/// reaches the draft and is highlighted until corrected or cancelled.
pub(crate) fn engineering_numeric_input_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut input_focus: ResMut<InputFocus>,
    mut keyboard_state: ResMut<UiKeyboardState>,
    mut state: ResMut<BoundaryLoadEditorState>,
    mut active_editor: ResMut<ActiveLoadEditor>,
    mut dload_kind: ResMut<SelectedDloadKind>,
    mut inputs: Query<(
        Entity,
        &EngineeringValueInput,
        &mut EditableText,
        &mut BorderColor,
    )>,
    mut statuses: Query<&mut Text, With<EngineeringInputStatus>>,
) {
    let focused_entity = input_focus.get();
    if !focused_entity.is_some_and(|entity| inputs.contains(entity)) {
        state.error = None;
    }
    for (entity, field, mut input, mut border) in &mut inputs {
        let focused = focused_entity == Some(entity);
        let current = editable_value(&input);

        if focused {
            if keys.just_pressed(KeyCode::Escape) {
                let restored = format_measurement(state.value(field.0));
                input.editor_mut().set_text(&restored);
                input.queue_edit(bevy::text::TextEdit::TextEnd(false));
                state.error = None;
                input_focus.clear();
                keyboard_state.text_editing = true;
            } else {
                match parse_measurement(&current) {
                    Ok(value) => {
                        state.set_value(field.0, value);
                        match field.0 {
                            EngineeringField::Constraint(dof) => {
                                state.constraint_enabled[usize::from(dof - 1)] = true;
                            }
                            EngineeringField::Nodal(_) => {
                                *active_editor = ActiveLoadEditor::Nodal;
                            }
                            EngineeringField::Pressure => {
                                *dload_kind = SelectedDloadKind::Pressure;
                                *active_editor = ActiveLoadEditor::Distributed;
                            }
                            EngineeringField::GravityAcceleration
                            | EngineeringField::GravityDirection(_) => {
                                *dload_kind = SelectedDloadKind::Gravity;
                                *active_editor = ActiveLoadEditor::Distributed;
                            }
                        }
                        state.error = None;
                        if keys.just_pressed(KeyCode::Enter) && !input.is_composing() {
                            input_focus.clear();
                            keyboard_state.text_editing = true;
                        }
                    }
                    Err(message) => state.error = Some((field.0, message.to_string())),
                }
            }
        } else {
            let desired = format_measurement(state.value(field.0));
            if current != desired {
                input.editor_mut().set_text(&desired);
                input.queue_edit(bevy::text::TextEdit::TextEnd(false));
            }
        }

        let field_has_error = state
            .error
            .as_ref()
            .is_some_and(|(error_field, _)| *error_field == field.0);
        *border = BorderColor::all(if field_has_error {
            INPUT_ERROR_BORDER
        } else if focused {
            INPUT_FOCUS_BORDER
        } else {
            INPUT_BORDER
        });
    }

    if let Ok(mut status) = statuses.single_mut() {
        **status = state
            .error
            .as_ref()
            .map(|(_, message)| format!("{message}; Esc restores the last valid value"))
            .unwrap_or_else(|| "Exact values are provisional until Apply".to_string());
    }
}

pub(crate) fn constraint_dof_toggle_system(
    mut state: ResMut<BoundaryLoadEditorState>,
    mut buttons: Query<(
        Ref<Interaction>,
        &ConstraintDofToggle,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (interaction, toggle, mut background, mut border) in &mut buttons {
        let index = usize::from(toggle.0.saturating_sub(1));
        if index >= 6 {
            continue;
        }
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            state.constraint_enabled[index] = !state.constraint_enabled[index];
        }
        let active = state.constraint_enabled[index];
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if active {
            INPUT_FOCUS_BORDER
        } else {
            INPUT_BORDER
        });
    }
}

pub(crate) fn rotational_input_mode_button_system(
    mut state: ResMut<BoundaryLoadEditorState>,
    mut buttons: Query<(
        Ref<Interaction>,
        &RotationalInputModeButton,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (interaction, button, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            match button.kind {
                RotationalInputKind::Constraint => {
                    state.constraint_rotation_mode = button.mode;
                }
                RotationalInputKind::Load => state.load_moment_mode = button.mode,
            }
            state.rotation_center_feedback = None;
        }
        let active = match button.kind {
            RotationalInputKind::Constraint => state.constraint_rotation_mode == button.mode,
            RotationalInputKind::Load => state.load_moment_mode == button.mode,
        };
        *background = BackgroundColor(match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        });
        *border = BorderColor::all(if active {
            INPUT_FOCUS_BORDER
        } else {
            INPUT_BORDER
        });
    }
}

pub(crate) fn rotation_center_button_system(
    model: Option<Res<FemModel>>,
    selection: Res<SelectionState>,
    mut state: ResMut<BoundaryLoadEditorState>,
    mut buttons: Query<(
        Ref<Interaction>,
        &RotationCenterButton,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (interaction, action, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            match action {
                RotationCenterButton::Capture => {
                    let selected_nodes: Vec<_> = selection
                        .targets
                        .iter()
                        .copied()
                        .filter(|target| matches!(target.entity, FemEntityId::Node(_)))
                        .collect();
                    if let [center] = selected_nodes.as_slice() {
                        let exists = model.as_deref().is_some_and(|model| {
                            let FemEntityId::Node(node) = center.entity else {
                                return false;
                            };
                            model
                                .meshes
                                .get(center.mesh_index)
                                .is_some_and(|mesh| mesh.node_position(node).is_some())
                        });
                        if exists {
                            state.rotation_center = Some(*center);
                            state.rotation_center_feedback = None;
                        } else {
                            state.rotation_center_feedback =
                                Some("Selected center is not present in the model".into());
                        }
                    } else {
                        state.rotation_center_feedback = Some(
                            "Select exactly one node, set the center, then select target nodes"
                                .into(),
                        );
                    }
                }
                RotationCenterButton::Clear => {
                    state.rotation_center = None;
                    state.rotation_center_feedback = None;
                }
            }
        }
        *background = BackgroundColor(match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        });
        *border = BorderColor::all(INPUT_BORDER);
    }
}

pub(crate) fn update_rotation_center_status(
    state: Res<BoundaryLoadEditorState>,
    mut statuses: Query<&mut Text, With<RotationCenterStatus>>,
) {
    if !state.is_changed() {
        return;
    }
    let text = if let Some(message) = &state.rotation_center_feedback {
        message.clone()
    } else if let Some(center) = state.rotation_center {
        match center.entity {
            FemEntityId::Node(node) => format!(
                "Center: part {} / node {} - now select target nodes",
                center.mesh_index + 1,
                node.0
            ),
            _ => "Center: invalid selection".to_string(),
        }
    } else {
        "Center: not set (required only for About center)".to_string()
    };
    for mut status in &mut statuses {
        **status = text.clone();
    }
}

pub(crate) fn apply_constraint_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    model: Option<Res<FemModel>>,
    selection: Res<SelectionState>,
    state: Res<BoundaryLoadEditorState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ApplyConstraintButton>,
    >,
) {
    let Some(model) = model else {
        return;
    };
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let needs_center = state.constraint_rotation_mode == RotationalInputMode::AboutCenter
                && state.constraint_enabled[3..].iter().any(|enabled| *enabled);
            if needs_center && state.resolved_rotation_center().is_none() {
                continue;
            }
            let mut added = false;
            for (mesh_index, nodes) in selected_nodes_by_mesh(&selection) {
                if nodes.is_empty() || model.meshes.get(mesh_index).is_none() {
                    continue;
                }
                let name = setup.next_auto_name_pub("BC");
                for (index, enabled) in state.constraint_enabled.iter().copied().enumerate() {
                    if !enabled {
                        continue;
                    }
                    let Some((dof, rotation_center)) = state.constraint_solver_component(index)
                    else {
                        continue;
                    };
                    setup.boundary_conditions.push(fem_core::BoundaryCondition {
                        name: name.clone(),
                        mesh_index,
                        nodes: nodes.clone(),
                        ngrp_name: None,
                        rotation_center,
                        dof_start: dof,
                        dof_end: dof,
                        value: state.constraint_values[index],
                    });
                    added = true;
                }
            }
            if added {
                setup.set_changed();
            }
        }
        *background = BackgroundColor(match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        });
        *border = BorderColor::all(INPUT_BORDER);
    }
}

pub(crate) fn update_apply_constraint_label(
    selection: Res<SelectionState>,
    state: Res<BoundaryLoadEditorState>,
    mut labels: Query<&mut Text, With<ApplyConstraintLabel>>,
) {
    if !selection.is_changed() && !state.is_changed() {
        return;
    }
    let Ok(mut label) = labels.single_mut() else {
        return;
    };
    let node_count = selection
        .targets
        .iter()
        .filter(|target| matches!(target.entity, FemEntityId::Node(_)))
        .count();
    let dof_count = state
        .constraint_enabled
        .iter()
        .filter(|enabled| **enabled)
        .count();
    let needs_center = state.constraint_rotation_mode == RotationalInputMode::AboutCenter
        && state.constraint_enabled[3..].iter().any(|enabled| *enabled)
        && state.rotation_center.is_none();
    **label = if needs_center {
        "Apply Constraints - set one rotation center".to_string()
    } else if node_count == 0 {
        "Apply Constraints - no nodes selected".to_string()
    } else if dof_count == 0 {
        format!("Apply Constraints - choose DOF ({node_count} nodes)")
    } else {
        format!("Apply {dof_count} DOF to {node_count} nodes")
    };
}

/// Axis buttons and sliders are quick manipulators. They update this exact
/// draft; direct typing also updates the selected axis when the force has a
/// single non-zero translational component.
pub(crate) fn sync_quick_load_controls(
    active_editor: Res<ActiveLoadEditor>,
    kind: Res<SelectedDloadKind>,
    mut state: ResMut<BoundaryLoadEditorState>,
    mut selected_direction: ResMut<SelectedLoadDirection>,
    mut quick: ResMut<QuickLoadControlState>,
    sliders: Query<&SliderState, With<SliderTrack>>,
) {
    let load_slider = sliders
        .iter()
        .find(|slider| slider.id == SliderId::LoadMagnitude);
    let dload_slider = sliders
        .iter()
        .find(|slider| slider.id == SliderId::DloadMagnitude);
    let load_value_changed = load_slider.is_some_and(|slider| {
        quick
            .load_value
            .replace(slider.value)
            .is_some_and(|previous| previous.to_bits() != slider.value.to_bits())
    });
    let dload_value_changed = dload_slider.is_some_and(|slider| {
        quick
            .dload_value
            .replace(slider.value)
            .is_some_and(|previous| previous.to_bits() != slider.value.to_bits())
    });

    match *active_editor {
        ActiveLoadEditor::Nodal => {
            if load_value_changed {
                if let Some((dof, sign)) = selected_direction.0 {
                    state.set_axis_force(dof, sign, load_slider.unwrap().value);
                }
            } else if state.is_changed() {
                selected_direction.0 = state.single_axis_force().map(|(direction, _)| direction);
            }
        }
        ActiveLoadEditor::Distributed => {
            if dload_value_changed {
                let slider = dload_slider.unwrap();
                match *kind {
                    SelectedDloadKind::Pressure => state.pressure = slider.value,
                    SelectedDloadKind::Gravity => state.gravity_acceleration = slider.value,
                }
            }
        }
        ActiveLoadEditor::None => {}
    }
}

pub(crate) fn update_dload_exact_field_visibility(
    kind: Res<SelectedDloadKind>,
    mut groups: Query<(&DloadExactFieldGroup, &mut Visibility)>,
) {
    if !kind.is_changed() {
        return;
    }
    for (group, mut visibility) in &mut groups {
        *visibility = if group.0 == *kind {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

fn selected_nodes_by_mesh(selection: &SelectionState) -> BTreeMap<usize, Vec<fem_core::NodeId>> {
    let mut by_mesh = BTreeMap::<usize, BTreeSet<fem_core::NodeId>>::new();
    for target in &selection.targets {
        if let FemEntityId::Node(node) = target.entity {
            by_mesh.entry(target.mesh_index).or_default().insert(node);
        }
    }
    by_mesh
        .into_iter()
        .map(|(mesh_index, nodes)| (mesh_index, nodes.into_iter().collect()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_enables_only_requested_dofs_and_zeros_them() {
        let mut state = BoundaryLoadEditorState::default();
        state.constraint_values = [1.0; 6];
        state.set_constraint_preset(1, 3);
        assert_eq!(
            state.constraint_enabled,
            [true, true, true, false, false, false]
        );
        assert_eq!(&state.constraint_values[..3], &[0.0, 0.0, 0.0]);
        assert_eq!(&state.constraint_values[3..], &[1.0, 1.0, 1.0]);
    }

    #[test]
    fn axis_force_is_signed_and_detectable() {
        let mut state = BoundaryLoadEditorState::default();
        state.set_axis_force(2, -1.0, 125.5);
        assert_eq!(state.translational_force(), Vec3::new(0.0, -125.5, 0.0));
        assert_eq!(state.single_axis_force(), Some(((2, -1.0), 125.5)));
    }

    #[test]
    fn general_force_vector_is_not_reduced_to_one_axis() {
        let mut state = BoundaryLoadEditorState::default();
        state.nodal_components[0] = 1.0;
        state.nodal_components[1] = 2.0;
        assert_eq!(state.single_axis_force(), None);
    }

    #[test]
    fn moment_components_form_a_rotation_axis() {
        let mut state = BoundaryLoadEditorState::default();
        state.nodal_components[3..].copy_from_slice(&[2.0, -3.0, 4.0]);
        assert_eq!(state.rotational_moment(), Vec3::new(2.0, -3.0, 4.0));
    }

    #[test]
    fn gravity_direction_must_be_nonzero() {
        let mut state = BoundaryLoadEditorState::default();
        assert_eq!(state.normalized_gravity_direction(), Some(Vec3::NEG_Y));
        state.gravity_direction = Vec3::ZERO;
        assert_eq!(state.normalized_gravity_direction(), None);
    }

    #[test]
    fn about_center_maps_ui_rotation_components_to_frontistr_dofs_one_to_three() {
        let mut state = BoundaryLoadEditorState::default();
        state.constraint_rotation_mode = RotationalInputMode::AboutCenter;
        state.load_moment_mode = RotationalInputMode::AboutCenter;
        state.rotation_center = Some(fem_core::FemEntityRef::node(2, fem_core::NodeId(7)));

        let constraint = state.constraint_solver_component(3).unwrap();
        let load = state.load_solver_component(5).unwrap();

        assert_eq!(constraint.0, 1);
        assert_eq!(constraint.1.unwrap().node, Some(fem_core::NodeId(7)));
        assert_eq!(load.0, 3);
        assert_eq!(load.1.unwrap().mesh_index, 2);
        assert_eq!(state.load_solver_component(0), Some((1, None)));
    }
}
