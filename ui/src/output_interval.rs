//! File output scheduling, separate from quantity menus and result playback.
use crate::{
    layout::{SidebarPage, UiInputCapture},
    measurement::editable_value,
};
use bevy::{
    input_focus::InputFocus,
    prelude::*,
    text::EditableText,
    ui_widgets::{Activate, Button as WidgetButton, SelectAllOnFocus},
};
use fem_core::{AnalysisSetup, AnalysisType, OutputTarget, UiKeyboardState};

#[derive(Component)]
struct Toggle(OutputTarget);
#[derive(Component)]
pub(crate) struct Label(OutputTarget);
#[derive(Component)]
pub(crate) struct Interval(OutputTarget);
#[derive(Component)]
pub(crate) struct Hint;

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent.spawn(label("SAVE RESULTS | next analysis"));
    for target in [OutputTarget::Res, OutputTarget::Vis] {
        parent
            .spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(5),
                min_height: px(28),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Button,
                    WidgetButton,
                    bevy::input_focus::tab_navigation::TabIndex(0),
                    Toggle(target),
                    Node {
                        width: px(90),
                        height: px(26),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.14, 0.30, 0.37)),
                ))
                .observe(toggle)
                .with_child((label(""), Label(target)));
                row.spawn(label("Every"));
                row.spawn((
                    Interval(target),
                    UiInputCapture,
                    SelectAllOnFocus,
                    Node {
                        flex_grow: 1.0,
                        min_width: px(40),
                        height: px(26),
                        padding: UiRect::all(px(4)),
                        border: UiRect::all(px(1)),
                        ..default()
                    },
                    EditableText {
                        allow_newlines: false,
                        max_characters: Some(10),
                        ..EditableText::new("1")
                    },
                    bevy::text::EditableTextFilter::new(|c| c.is_ascii_digit()),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    BorderColor::all(Color::srgb(0.34, 0.40, 0.44)),
                    BackgroundColor(Color::srgb(0.075, 0.09, 0.10)),
                ));
                row.spawn(label("steps"));
            });
    }
    parent.spawn((label(""), Hint));
}

fn label(value: &str) -> impl Bundle {
    (
        Text::new(value),
        bevy::picking::Pickable::IGNORE,
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(Color::WHITE),
    )
}

fn toggle(
    event: On<Activate>,
    buttons: Query<&Toggle>,
    page: Res<SidebarPage>,
    mut setup: ResMut<AnalysisSetup>,
) {
    if *page != SidebarPage::Solve {
        return;
    }
    if let Ok(button) = buttons.get(event.entity) {
        let control = setup.output.write_mut(button.0);
        control.enabled = !control.enabled;
    }
}

fn parse(value: &str) -> Option<u32> {
    value
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|v| *v > 0 && *v <= i32::MAX as u32)
}

pub(crate) fn update(
    keys: Res<ButtonInput<KeyCode>>,
    page: Res<SidebarPage>,
    mut focus: ResMut<InputFocus>,
    mut keyboard: ResMut<UiKeyboardState>,
    mut setup: ResMut<AnalysisSetup>,
    mut inputs: Query<(
        Entity,
        &Interval,
        &mut EditableText,
        &mut BorderColor,
        &mut TextColor,
    )>,
    mut labels: Query<(&Label, &mut Text), Without<Hint>>,
    mut hints: Query<&mut Text, (With<Hint>, Without<Label>)>,
) {
    let eigen = setup.solver.analysis_type == AnalysisType::Eigen;
    let mut error = false;
    for (entity, target, mut input, mut border, mut color) in &mut inputs {
        let control = *setup.output.write(target.0);
        // FrontISTR's eigen output loop does not consult WRITE FREQUENCY.
        let active = *page == SidebarPage::Solve && control.enabled && !eigen;
        color.0 = if active {
            Color::WHITE
        } else {
            Color::srgb(0.45, 0.50, 0.53)
        };
        if !active && focus.get() == Some(entity) {
            focus.clear();
        }
        let focused = focus.get() == Some(entity);
        let value = editable_value(&input);
        let mut invalid = false;
        if focused && !keys.just_pressed(KeyCode::Escape) {
            let parsed = parse(&value);
            invalid = parsed.is_none();
            if keys.just_pressed(KeyCode::Enter) && !input.is_composing() {
                if let Some(value) = parsed {
                    setup.output.write_mut(target.0).frequency = value;
                    focus.clear();
                    keyboard.text_editing = true;
                }
            }
        } else {
            let desired = control.frequency.to_string();
            if value != desired {
                input.editor_mut().set_text(&desired);
            }
            if focused {
                focus.clear();
                keyboard.text_editing = true;
            }
        }
        error |= invalid;
        *border = BorderColor::all(if invalid {
            Color::srgb(0.98, 0.36, 0.30)
        } else if focused {
            Color::srgb(0.57, 0.86, 0.92)
        } else {
            Color::srgb(0.34, 0.40, 0.44)
        });
    }
    for (target, mut text) in &mut labels {
        text.0 = format!(
            "{}: {}",
            target.0.label(),
            if setup.output.write(target.0).enabled {
                "ON"
            } else {
                "OFF"
            }
        );
    }
    for mut hint in &mut hints {
        hint.0 = if error { "Interval: 1..2147483647. Enter applies; Esc restores." }
            else if eigen { "Eigen: all requested modes; intervals are not used. ON/OFF still applies." }
            else { "Solver steps, not seconds or playback speed. Enter applies; Esc restores. OFF: no file requested." }.into();
    }
}

#[cfg(test)]
#[path = "output_interval_tests.rs"]
mod tests;
