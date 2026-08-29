//! Shared numeric confirmation for direct viewport operations.
//!
//! A direct manipulation should stay fast and visual, while its engineering
//! value remains exact and inspectable.  This module provides a small
//! SketchUp-style measurement box in the lower-right corner of the viewport.
//! The first consumer is assembly translation; later load, contact, clipping,
//! and probe tools can reuse the same transaction surface.

use crate::layout::UiInputCapture;
use crate::slider::{SliderId, SliderState, SliderTrack};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, TextCursorStyle};
use bevy::ui_widgets::SelectAllOnFocus;
use fem_core::{ContactCandidateState, FemModel, FemModelVersion, UiKeyboardState, ViewportTool};

const BOX_BG: Color = Color::srgba(0.035, 0.04, 0.045, 0.96);
const BOX_BORDER: Color = Color::srgba(0.38, 0.48, 0.54, 0.92);
const INPUT_BG: Color = Color::srgba(0.075, 0.09, 0.10, 0.98);
const INPUT_FOCUS_BORDER: Color = Color::srgb(0.57, 0.86, 0.92);
const INPUT_ERROR_BORDER: Color = Color::srgb(0.98, 0.36, 0.30);
const TEXT_MAIN: Color = Color::srgb(0.90, 0.94, 0.96);
const TEXT_MUTED: Color = Color::srgb(0.60, 0.68, 0.72);

#[derive(Debug, Clone, Copy)]
pub(crate) enum MeasurementTarget {
    AssemblyTranslation {
        part_index: usize,
        axis: Vec3,
        committed_value: f32,
    },
    AssemblyRotation {
        part_index: usize,
        axis: Vec3,
        committed_degrees: f32,
    },
    SliderValue {
        slider_id: SliderId,
        label: &'static str,
        units: &'static str,
    },
}

#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct MeasurementBoxState {
    pub target: Option<MeasurementTarget>,
    pub value: f32,
    pub dragging: bool,
    pub error: Option<String>,
}

impl MeasurementBoxState {
    pub fn begin_assembly_translation(&mut self, part_index: usize, axis: Vec3) {
        self.target = Some(MeasurementTarget::AssemblyTranslation {
            part_index,
            axis,
            committed_value: 0.0,
        });
        self.value = 0.0;
        self.dragging = true;
        self.error = None;
    }

    pub fn preview_translation(&mut self, value: f32) {
        if self.dragging && value.is_finite() {
            self.value = value;
        }
    }

    pub fn commit_translation(&mut self, value: f32) {
        let Some(MeasurementTarget::AssemblyTranslation {
            committed_value, ..
        }) = self.target.as_mut()
        else {
            return;
        };
        *committed_value = value;
        self.value = value;
        self.dragging = false;
        self.error = None;
    }

    pub fn begin_assembly_rotation(&mut self, part_index: usize, axis: Vec3) {
        self.target = Some(MeasurementTarget::AssemblyRotation {
            part_index,
            axis,
            committed_degrees: 0.0,
        });
        self.value = 0.0;
        self.dragging = true;
        self.error = None;
    }

    pub fn preview_rotation(&mut self, degrees: f32) {
        if self.dragging && degrees.is_finite() {
            self.value = degrees;
        }
    }

    pub fn commit_rotation(&mut self, degrees: f32) {
        let Some(MeasurementTarget::AssemblyRotation {
            committed_degrees, ..
        }) = self.target.as_mut()
        else {
            return;
        };
        *committed_degrees = degrees;
        self.value = degrees;
        self.dragging = false;
        self.error = None;
    }

    pub fn begin_slider_value(
        &mut self,
        slider_id: SliderId,
        label: &'static str,
        units: &'static str,
        value: f32,
    ) {
        self.target = Some(MeasurementTarget::SliderValue {
            slider_id,
            label,
            units,
        });
        self.value = value;
        self.dragging = false;
        self.error = None;
    }

    pub fn update_slider_value(&mut self, slider_id: SliderId, value: f32) {
        if matches!(
            self.target,
            Some(MeasurementTarget::SliderValue {
                slider_id: target,
                ..
            }) if target == slider_id
        ) && value.is_finite()
        {
            self.value = value;
            self.error = None;
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Component)]
pub(crate) struct MeasurementBoxRoot;

#[derive(Component)]
pub(crate) struct MeasurementLabel;

#[derive(Component)]
pub(crate) struct MeasurementValueInput;

#[derive(Component)]
pub(crate) struct MeasurementStatus;

pub(crate) fn spawn_measurement_box(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(14.0),
                bottom: px(14.0),
                width: px(286.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(5.0),
                padding: UiRect::all(px(9.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(6.0)),
                ..default()
            },
            BackgroundColor(BOX_BG),
            BorderColor::all(BOX_BORDER),
            GlobalZIndex(100),
            Visibility::Hidden,
            UiInputCapture,
            MeasurementBoxRoot,
            Name::new("Viewport measurement box"),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("Distance X"),
                TextFont {
                    font_size: FontSize::Px(11.5),
                    ..default()
                },
                TextColor(TEXT_MAIN),
                MeasurementLabel,
            ));

            root.spawn((
                Node {
                    width: percent(100.0),
                    min_height: px(30.0),
                    padding: UiRect::axes(px(8.0), px(5.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(4.0)),
                    ..default()
                },
                EditableText {
                    visible_width: Some(20.0),
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
                    font_size: FontSize::Px(15.0),
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
                BorderColor::all(BOX_BORDER),
                MeasurementValueInput,
                Name::new("Viewport measurement value"),
            ));

            root.spawn((
                Text::new("model units  |  click value, Enter = apply exact distance"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                MeasurementStatus,
            ));
        });
}

/// Records whether any Bevy [`EditableText`] currently owns keyboard focus.
/// Shortcut systems in other plugins read this shared resource so typing a
/// value cannot accidentally change the selection filter, result step, or
/// undo the analysis setup.
pub(crate) fn update_ui_keyboard_state(
    input_focus: Option<Res<InputFocus>>,
    editable_text: Query<(), With<EditableText>>,
    mut keyboard_state: ResMut<UiKeyboardState>,
) {
    keyboard_state.text_editing = input_focus
        .as_deref()
        .and_then(InputFocus::get)
        .is_some_and(|entity| editable_text.contains(entity));
}

/// Applies or cancels an exact numeric correction for the most recent
/// viewport transaction.  Mouse release has already committed the visual
/// drag; submitting replaces that signed distance by applying only the
/// difference, matching SketchUp's post-operation numeric entry.
pub(crate) fn measurement_box_input_system(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut input_focus: ResMut<InputFocus>,
    mut keyboard_state: ResMut<UiKeyboardState>,
    mut state: ResMut<MeasurementBoxState>,
    mut model: Option<ResMut<FemModel>>,
    mut version: ResMut<FemModelVersion>,
    mut contact_candidates: ResMut<ContactCandidateState>,
    mut sliders: Query<&mut SliderState, With<SliderTrack>>,
    mut input_query: Query<(Entity, &mut EditableText), With<MeasurementValueInput>>,
    root_query: Query<(&ComputedNode, &UiGlobalTransform), With<MeasurementBoxRoot>>,
) {
    let Ok((input_entity, input)) = input_query.single_mut() else {
        return;
    };
    if input_focus.get() != Some(input_entity) {
        return;
    }

    // Clicking back into the viewport abandons unsubmitted text while still
    // allowing that very click to start the next viewport gesture.
    if buttons.just_pressed(MouseButton::Left) && !cursor_is_over_root(&windows, &root_query) {
        input_focus.clear();
        state.error = None;
        return;
    }

    if keys.just_pressed(KeyCode::Escape) {
        input_focus.clear();
        state.error = None;
        // Keep text-edit capture true for the rest of this frame so Escape
        // is not also consumed by selection or another global shortcut.
        keyboard_state.text_editing = true;
        return;
    }

    if !keys.just_pressed(KeyCode::Enter) || input.is_composing() {
        return;
    }

    let text = editable_value(&input);
    let requested = match parse_measurement(&text) {
        Ok(value) => value,
        Err(message) => {
            state.error = Some(message.to_string());
            return;
        }
    };

    let Some(target) = state.target else {
        state.error = Some("No viewport operation to update".to_string());
        return;
    };
    let changed = match target {
        MeasurementTarget::AssemblyTranslation {
            part_index,
            axis,
            committed_value,
        } => {
            let correction = assembly_translation_correction(axis, committed_value, requested);
            if correction.length_squared() <= f32::EPSILON * f32::EPSILON {
                false
            } else {
                let Some(model) = model.as_deref_mut() else {
                    state.error = Some("No model is loaded".to_string());
                    return;
                };
                if !model.translate_part(part_index, correction) {
                    state.error = Some("The selected part can no longer be updated".to_string());
                    return;
                }
                true
            }
        }
        MeasurementTarget::AssemblyRotation {
            part_index,
            axis,
            committed_degrees,
        } => {
            let correction_degrees = requested - committed_degrees;
            if correction_degrees.abs() <= f32::EPSILON {
                false
            } else {
                let Some(model) = model.as_deref_mut() else {
                    state.error = Some("No model is loaded".to_string());
                    return;
                };
                if !model.rotate_part_about_centroid(
                    part_index,
                    Quat::from_axis_angle(axis, correction_degrees.to_radians()),
                ) {
                    state.error = Some("The selected part can no longer be updated".to_string());
                    return;
                }
                true
            }
        }
        MeasurementTarget::SliderValue { slider_id, .. } => {
            let Some(mut slider) = sliders.iter_mut().find(|slider| slider.id == slider_id) else {
                state.error = Some("The setting is no longer available".to_string());
                return;
            };
            slider.value = requested;
            false
        }
    };

    if changed {
        contact_candidates.candidates.clear();
        contact_candidates.selected = None;
        version.bump();
    }
    match target {
        MeasurementTarget::AssemblyTranslation { .. } => state.commit_translation(requested),
        MeasurementTarget::AssemblyRotation { .. } => state.commit_rotation(requested),
        MeasurementTarget::SliderValue { slider_id, .. } => {
            state.update_slider_value(slider_id, requested)
        }
    }
    input_focus.clear();
    // Preserve capture until the next frame so Enter is not reused by a
    // global shortcut registered later in the schedule.
    keyboard_state.text_editing = true;
}

pub(crate) fn update_measurement_box_visuals(
    tool: Res<ViewportTool>,
    mut state: ResMut<MeasurementBoxState>,
    input_focus: Res<InputFocus>,
    mut roots: Query<&mut Visibility, With<MeasurementBoxRoot>>,
    mut labels: Query<&mut Text, (With<MeasurementLabel>, Without<MeasurementStatus>)>,
    mut inputs: Query<(Entity, &mut EditableText, &mut BorderColor), With<MeasurementValueInput>>,
    mut statuses: Query<&mut Text, (With<MeasurementStatus>, Without<MeasurementLabel>)>,
) {
    let visible = state.target.is_some_and(|target| match target {
        MeasurementTarget::AssemblyTranslation { .. }
        | MeasurementTarget::AssemblyRotation { .. } => *tool == ViewportTool::Assembly,
        MeasurementTarget::SliderValue { .. } => *tool == ViewportTool::Selection,
    });
    if let Ok(mut visibility) = roots.single_mut() {
        *visibility = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !visible {
        return;
    }

    let Some(target) = state.target else {
        return;
    };
    let (label_text, units, value_name) = match target {
        MeasurementTarget::AssemblyTranslation { axis, .. } => (
            format!("Distance {}", axis_label(axis)),
            "model units",
            "distance",
        ),
        MeasurementTarget::AssemblyRotation { axis, .. } => {
            (format!("Angle R{}", axis_label(axis)), "degrees", "angle")
        }
        MeasurementTarget::SliderValue { label, units, .. } => (label.to_string(), units, "value"),
    };
    if let Ok(mut label) = labels.single_mut() {
        **label = label_text;
    }

    let Ok((input_entity, mut input, mut border)) = inputs.single_mut() else {
        return;
    };
    let focused = input_focus.get() == Some(input_entity);
    let current_text = editable_value(&input);

    if focused {
        state.error = parse_measurement(&current_text).err().map(str::to_string);
    } else {
        let desired = format_measurement(state.value);
        if current_text != desired {
            input.editor_mut().set_text(&desired);
            input.queue_edit(bevy::text::TextEdit::TextEnd(false));
        }
    }

    let has_error = state.error.is_some();
    *border = BorderColor::all(if has_error {
        INPUT_ERROR_BORDER
    } else if focused {
        INPUT_FOCUS_BORDER
    } else {
        BOX_BORDER
    });

    if let Ok(mut status) = statuses.single_mut() {
        **status = if let Some(error) = state.error.as_deref() {
            format!("{error}  |  Esc = restore")
        } else if state.dragging {
            format!("{units}  |  drag now; Shift = fine, Ctrl = snap")
        } else if focused {
            format!("{units}  |  Enter = apply exact {value_name}, Esc = restore")
        } else {
            format!("{units}  |  click value, Enter = apply exact {value_name}")
        };
    }
}

fn cursor_is_over_root(
    windows: &Query<&Window>,
    roots: &Query<(&ComputedNode, &UiGlobalTransform), With<MeasurementBoxRoot>>,
) -> bool {
    let Some(cursor) = windows.single().ok().and_then(Window::cursor_position) else {
        return false;
    };
    roots.iter().any(|(node, transform)| {
        let scale = node.inverse_scale_factor;
        let size = node.size() * scale;
        let origin = transform.transform_point2(Vec2::ZERO) * scale - size * 0.5;
        cursor.x >= origin.x
            && cursor.x <= origin.x + size.x
            && cursor.y >= origin.y
            && cursor.y <= origin.y + size.y
    })
}

fn editable_value(input: &EditableText) -> String {
    input.value().into_iter().collect()
}

fn parse_measurement(text: &str) -> Result<f32, &'static str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Enter a value");
    }
    let value = trimmed.parse::<f64>().map_err(|_| "Enter a valid number")?;
    if !value.is_finite() || value.abs() > f32::MAX as f64 {
        return Err("Value is outside the supported range");
    }
    Ok(value as f32)
}

fn format_measurement(value: f32) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if value.abs() >= 1.0e7 || value.abs() < 1.0e-5 {
        return format!("{value:.6e}");
    }
    let mut text = format!("{value:.6}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn axis_label(axis: Vec3) -> &'static str {
    if axis.abs().max_element() == axis.x.abs() {
        "X"
    } else if axis.abs().max_element() == axis.y.abs() {
        "Y"
    } else {
        "Z"
    }
}

fn assembly_translation_correction(axis: Vec3, committed: f32, requested: f32) -> Vec3 {
    axis * (requested - committed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_parser_accepts_signed_decimal_and_exponent() {
        assert_eq!(parse_measurement("-12.5"), Ok(-12.5));
        assert_eq!(parse_measurement("2.5e2"), Ok(250.0));
    }

    #[test]
    fn measurement_parser_rejects_invalid_and_non_finite_values() {
        assert!(parse_measurement("").is_err());
        assert!(parse_measurement("1.2.3").is_err());
        assert!(parse_measurement("1e999").is_err());
    }

    #[test]
    fn measurement_formatter_is_compact_without_losing_the_sign() {
        assert_eq!(format_measurement(12.5), "12.5");
        assert_eq!(format_measurement(-0.125), "-0.125");
        assert_eq!(format_measurement(0.0), "0");
    }

    #[test]
    fn numeric_override_applies_only_the_difference_from_the_committed_drag() {
        assert_eq!(
            assembly_translation_correction(Vec3::Y, 12.0, 10.5),
            Vec3::new(0.0, -1.5, 0.0)
        );
    }

    #[test]
    fn rotation_measurement_tracks_degrees_for_numeric_override() {
        let mut state = MeasurementBoxState::default();
        state.begin_assembly_rotation(2, Vec3::Z);
        state.preview_rotation(32.5);
        assert_eq!(state.value, 32.5);
        assert!(state.dragging);

        state.commit_rotation(30.0);
        assert_eq!(state.value, 30.0);
        assert!(!state.dragging);
        let Some(MeasurementTarget::AssemblyRotation {
            part_index,
            axis,
            committed_degrees,
        }) = state.target
        else {
            panic!("rotation target");
        };
        assert_eq!(part_index, 2);
        assert_eq!(axis, Vec3::Z);
        assert_eq!(committed_degrees, 30.0);
    }

    #[test]
    fn slider_measurement_tracks_an_exact_engineering_value() {
        let mut state = MeasurementBoxState::default();
        state.begin_slider_value(
            SliderId::LoadMagnitude,
            "Nodal load +X",
            "analysis units",
            100.0,
        );
        state.update_slider_value(SliderId::LoadMagnitude, 1250.5);

        assert_eq!(state.value, 1250.5);
        assert!(matches!(
            state.target,
            Some(MeasurementTarget::SliderValue {
                slider_id: SliderId::LoadMagnitude,
                ..
            })
        ));
    }
}
