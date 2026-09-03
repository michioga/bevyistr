//! Layout for the contiguous part-position workflow.
use super::*;
use crate::assembly_clearance::spawn_assembly_clearance_ui;
use crate::layout::ScrollableList;
use crate::slider::{SliderConfig, spawn_slider};
use bevy::ui::ScrollPosition;

pub(crate) fn spawn_assembly_ui(parent: &mut ChildSpawnerCommands) {
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

    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(6.0),
                ..default()
            },
            Name::new("AssemblyTransformControls"),
        ))
        .with_children(spawn_transform_controls);

    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(6.0),
                margin: UiRect::top(px(6.0)),
                padding: UiRect::top(px(10.0)),
                border: UiRect::top(px(1.0)),
                ..default()
            },
            BorderColor::all(PANEL_BORDER),
            Name::new("AssemblyClearanceSection"),
        ))
        .with_children(|section| {
            hint_text(section, "CLEARANCE");
            spawn_assembly_clearance_ui(section);
        });
}

fn spawn_transform_controls(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                ..default()
            },
            Name::new("AssemblyToolBar"),
        ))
        .with_children(|row| {
            for (index, choice) in AssemblyToolChoice::ALL.into_iter().enumerate() {
                let (radius, border) = segment_style(index == 0, index == 2);
                row.spawn((
                    Button,
                    Node {
                        flex_grow: 1.0,
                        height: px(29.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border,
                        border_radius: radius,
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(PANEL_BORDER),
                    AssemblyToolButton { choice },
                    Name::new(format!("AssemblyTool_{}", choice.label())),
                ))
                .with_child((
                    Text::new(choice.label()),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));
            }
        });
    parent.spawn((
        Text::new(AssemblyToolChoice::Select.hint()),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(TEXT_MUTED),
        AssemblyToolHint,
    ));
    for mode in AssemblyGizmoMode::ALL {
        parent
            .spawn((
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6.0),
                    ..default()
                },
                AssemblyNudgeControls(Some(mode)),
                Name::new(format!("AssemblyNudgeControls_{}", mode.label())),
            ))
            .with_children(|controls| {
                let config = match mode {
                    AssemblyGizmoMode::Move => SliderConfig {
                        width: 272.0,
                        min: 0.1,
                        max: 10.0,
                        value: 1.0,
                        label: "Move step (% of part size)",
                        id: SliderId::AssemblyMovePercent,
                    },
                    AssemblyGizmoMode::Rotate => SliderConfig {
                        width: 272.0,
                        min: 1.0,
                        max: 45.0,
                        value: 5.0,
                        label: "Rotate step (deg)",
                        id: SliderId::AssemblyRotationDegrees,
                    },
                };
                spawn_slider(controls, config);
                controls.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                    AssemblyStepStatusText(mode),
                    Name::new(format!("AssemblyStepInfo_{}", mode.label())),
                ));
                assembly_axis_nudge_row(controls, mode);
                hint_text(
                    controls,
                    match mode {
                        AssemblyGizmoMode::Move => "-/+ step along world X/Y/Z axes",
                        AssemblyGizmoMode::Rotate => {
                            "RX/RY/RZ: world axes, part center, right-hand rule"
                        }
                    },
                );
            });
    }

    parent
        .spawn((
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: px(6.0),
                ..default()
            },
            AssemblyNudgeControls(None),
            Name::new("AssemblyPoseActions"),
        ))
        .with_children(|actions| {
            action_button(
                actions,
                "Reset selected part pose",
                AssemblyTransformButton {
                    action: AssemblyTransformAction::Reset,
                },
                "AssemblyResetPoseButton",
                BUTTON_NORMAL,
                PANEL_BORDER,
            );
            hint_text(
                actions,
                "Position changes clear contact candidates; run Detect again",
            );
        });
}

fn assembly_axis_nudge_row(parent: &mut ChildSpawnerCommands, mode: AssemblyGizmoMode) {
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(5.0),
            ..default()
        },))
        .with_children(|row| {
            let labels = match mode {
                AssemblyGizmoMode::Move => ["X", "Y", "Z"],
                AssemblyGizmoMode::Rotate => ["RX", "RY", "RZ"],
            };
            for (axis, label) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().zip(labels) {
                let action = AssemblyTransformAction::for_axis(mode, axis);
                let (background, border) = transform_button_colors(action, Interaction::None);
                row.spawn((
                    Node {
                        flex_grow: 1.0,
                        flex_basis: px(0.0),
                        flex_direction: FlexDirection::Row,
                        min_width: px(0.0),
                        height: px(30.0),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(5.0)),
                        ..default()
                    },
                    BackgroundColor(background),
                    BorderColor::all(border),
                    Name::new(format!("AssemblyAxis_{label}")),
                ))
                .with_children(|group| {
                    axis_step_button(group, mode, -axis, label, "-", background);
                    // The center identifies the axis; it is not a third action.
                    group
                        .spawn((
                            Node {
                                width: px(28.0),
                                flex_shrink: 0.0,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::axes(px(1.0), px(0.0)),
                                ..default()
                            },
                            BorderColor::all(border),
                        ))
                        .with_child((
                            Text::new(label),
                            TextFont {
                                font_size: FontSize::Px(11.5),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    axis_step_button(group, mode, axis, label, "+", background);
                });
            }
        });
}

fn axis_step_button(
    parent: &mut ChildSpawnerCommands,
    mode: AssemblyGizmoMode,
    axis: Vec3,
    label: &str,
    sign: &'static str,
    background: Color,
) {
    parent
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                flex_basis: px(0.0),
                min_width: px(26.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: if sign == "-" {
                    BorderRadius::new(px(4.0), px(0.0), px(0.0), px(4.0))
                } else {
                    BorderRadius::new(px(0.0), px(4.0), px(4.0), px(0.0))
                },
                ..default()
            },
            BackgroundColor(background),
            BorderColor::all(PANEL_BORDER),
            AssemblyTransformButton {
                action: AssemblyTransformAction::for_axis(mode, axis),
            },
            Name::new(format!("AssemblyStep_{sign}{label}")),
        ))
        .with_child((
            Text::new(sign),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));
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
