//! Axis presets and numeric controls for the Results section.
use crate::{
    InteractionButton,
    slider::{SliderConfig, SliderId, SliderState, SliderTrack, spawn_slider},
};
use bevy::{
    input_focus::tab_navigation::TabIndex,
    prelude::*,
    ui_widgets::{Activate, Button as WidgetButton},
};
use fem_core::FemResultSet;
use visualization::{ResultSectionDisplay, ResultSectionSettings, SectionStatus};

#[derive(Component)]
enum SectionPanel {
    Root,
    Options,
}
#[derive(Component, Clone, Copy)]
enum SectionButton {
    Toggle,
    Exterior,
    Axis(f32, f32),
}
#[derive(Component)]
struct SectionStatusText;

pub(crate) fn register(app: &mut App) {
    app.init_resource::<ResultSectionSettings>()
        .init_resource::<ResultSectionDisplay>()
        .add_systems(Update, sync);
}

fn label(value: &str) -> impl Bundle {
    (
        Text::new(value),
        Pickable::IGNORE,
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
    )
}

fn button(parent: &mut ChildSpawnerCommands, action: SectionButton, text: &str) {
    parent
        .spawn((
            InteractionButton,
            WidgetButton,
            TabIndex(0),
            action,
            Node {
                min_height: px(26),
                flex_grow: 1.0,
                flex_basis: px(0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.35, 0.55, 0.62)),
            BackgroundColor(Color::srgb(0.10, 0.14, 0.17)),
        ))
        .observe(activate)
        .with_child(label(text));
}

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    let column = || Node {
        width: percent(100),
        display: Display::None,
        flex_direction: FlexDirection::Column,
        row_gap: px(4),
        ..default()
    };
    parent
        .spawn((column(), SectionPanel::Root))
        .with_children(|panel| {
            button(panel, SectionButton::Toggle, "Section view");
            panel
                .spawn((column(), SectionPanel::Options))
                .with_children(|content| {
                    content
                        .spawn(Node {
                            width: percent(100),
                            column_gap: px(4),
                            ..default()
                        })
                        .with_children(|row| {
                            button(row, SectionButton::Axis(0.0, 0.0), "X");
                            button(row, SectionButton::Axis(90.0, 0.0), "Y");
                            button(row, SectionButton::Axis(0.0, 90.0), "Z");
                        });
                    for (id, label, min, max, value) in [
                        (SliderId::SectionPosition, "Position (%)", 0.0, 100.0, 50.0),
                        (
                            SliderId::SectionAzimuth,
                            "Azimuth (deg)",
                            -180.0,
                            180.0,
                            0.0,
                        ),
                        (
                            SliderId::SectionElevation,
                            "Elevation (deg)",
                            -90.0,
                            90.0,
                            90.0,
                        ),
                    ] {
                        spawn_slider(
                            content,
                            SliderConfig {
                                width: 270.0,
                                min,
                                max,
                                value,
                                label,
                                id,
                            },
                        );
                    }
                    button(content, SectionButton::Exterior, "Show exterior edges");
                    content.spawn((label(""), SectionStatusText));
                });
        });
}

fn activate(
    event: On<Activate>,
    actions: Query<&SectionButton>,
    results: Res<FemResultSet>,
    mut setup: ResMut<ResultSectionSettings>,
    mut sliders: Query<&mut SliderState, With<SliderTrack>>,
) {
    if results.active_field().is_none() {
        return;
    }
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    match *action {
        SectionButton::Toggle => setup.enabled = !setup.enabled,
        SectionButton::Exterior => setup.exterior = !setup.exterior,
        SectionButton::Axis(azimuth, elevation) => {
            for mut slider in &mut sliders {
                match slider.id {
                    SliderId::SectionAzimuth => slider.value = azimuth,
                    SliderId::SectionElevation => slider.value = elevation,
                    _ => {}
                }
            }
        }
    }
}

fn normal_from_angles(azimuth: f32, elevation: f32) -> Vec3 {
    let (az_sin, az_cos) = azimuth.to_radians().sin_cos();
    let (el_sin, el_cos) = elevation.to_radians().sin_cos();
    Vec3::from_array([el_cos * az_cos, el_cos * az_sin, el_sin].map(|component| {
        if component.abs() < 1e-7 {
            0.0
        } else {
            component
        }
    }))
}

fn sync(
    results: Res<FemResultSet>,
    mut setup: ResMut<ResultSectionSettings>,
    display: Res<ResultSectionDisplay>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut panels: Query<(&SectionPanel, &mut Node)>,
    mut buttons: Query<(&SectionButton, &mut BackgroundColor)>,
    mut status: Query<&mut Text, With<SectionStatusText>>,
) {
    let has_result = results.active_field().is_some();
    if setup.enabled && has_result {
        let (mut fraction, mut azimuth, mut elevation) = (0.5, 0.0, 90.0);
        for slider in &sliders {
            match slider.id {
                SliderId::SectionPosition => fraction = slider.value / 100.0,
                SliderId::SectionAzimuth => azimuth = slider.value,
                SliderId::SectionElevation => elevation = slider.value,
                _ => {}
            }
        }
        let normal = normal_from_angles(azimuth, elevation);
        let next = ResultSectionSettings {
            normal,
            fraction,
            ..*setup
        };
        setup.set_if_neq(next);
    }
    for (layer, mut node) in &mut panels {
        let visible = has_result && (matches!(layer, SectionPanel::Root) || setup.enabled);
        let style = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != style {
            node.display = style;
        }
    }
    for (button, mut background) in &mut buttons {
        let selected = match *button {
            SectionButton::Toggle => setup.enabled,
            SectionButton::Exterior => setup.exterior,
            SectionButton::Axis(azimuth, elevation) => {
                setup.normal == normal_from_angles(azimuth, elevation)
            }
        };
        background.set_if_neq(BackgroundColor(if selected {
            Color::srgb(0.18, 0.45, 0.55)
        } else {
            Color::srgb(0.10, 0.14, 0.17)
        }));
    }
    for mut text in &mut status {
        text.set_if_neq(Text::new(match display.status {
            SectionStatus::Inactive => "",
            SectionStatus::NeedsTetrahedra => {
                "A section needs a first-order tetrahedral volume mesh."
            }
            SectionStatus::NeedsNodalField => "Choose a nodal field for the section.",
            SectionStatus::Empty => "No section values at this position and frame.",
            SectionStatus::Visible => "Reference geometry | surface probe paused",
        }));
    }
}
