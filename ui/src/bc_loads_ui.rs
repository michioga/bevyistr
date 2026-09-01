use crate::boundary_editor::{
    BoundaryLoadEditorState, RotationalInputMode, spawn_constraint_exact_editor,
    spawn_dload_exact_editor, spawn_engineering_input_status, spawn_nodal_exact_editor,
};
use crate::layout::{DeleteSetupEntry, SidebarPage, SidebarPageContent};
use crate::load_direction::{LoadDirectionPickerButton, LoadDirectionPickerLabel};
use crate::materials_ui::setup_entry_row;
use crate::measurement::{MeasurementBoxState, MeasurementTarget};
use crate::slider::{SliderConfig, SliderId, SliderState, SliderTrack, spawn_slider};
use bevy::prelude::*;
use fem_core::{FemEntityId, FemModel};
use selection::SelectionState;
use std::collections::{BTreeMap, BTreeSet};
use visualization::{
    BoundaryLoadPreview, BoundaryLoadPreviewArrow, BoundaryLoadPreviewKind,
    BoundaryLoadPreviewMoment,
};

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);

#[derive(Component)]
pub(crate) struct ToggleConstraintsButton;

#[derive(Component)]
pub(crate) struct ToggleLoadsButton;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ConstraintPresetButton {
    pub dof_start: u8,
    pub dof_end: u8,
    pub label: &'static str,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct LoadDirectionButton {
    pub dof: u8,
    pub sign: f32,
}

#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct SelectedLoadDirection(pub Option<(u8, f32)>);

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ActiveLoadEditor {
    #[default]
    None,
    Nodal,
    Distributed,
}

#[derive(Component)]
pub(crate) struct ApplyLoadButton;

#[derive(Component)]
pub(crate) struct ConstraintPresetLabel;

#[derive(Component)]
pub(crate) struct ApplyLoadLabel;

#[derive(Component)]
pub(crate) struct ClearAllBcLoadsButton;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SelectedDloadKind {
    #[default]
    Pressure,
    Gravity,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct DloadKindButton(pub SelectedDloadKind);

#[derive(Component)]
pub(crate) struct ApplyDloadButton;

#[derive(Component)]
pub(crate) struct ApplyDloadLabel;

#[derive(Component)]
pub(crate) struct BoundaryLoadsListContainer;

pub(crate) fn spawn_boundary_load_editor(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(6.0),
                ..default()
            },
            SidebarPageContent::page(SidebarPage::Loads),
        ))
        .with_children(|row| {
            action_button(
                row,
                "Constraints",
                ToggleConstraintsButton,
                "ToggleConstraintsButton",
            );
            action_button(row, "Loads", ToggleLoadsButton, "ToggleLoadsButton");
        });

    parent.spawn((
        Text::new(
            "Red cone = fixed U   Magenta ring = fixed R\n\
             Orange arrow = force   Orange arc = moment",
        ),
        TextFont {
            font_size: FontSize::Px(9.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
        SidebarPageContent::page(SidebarPage::Loads),
    ));

    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(5.0),
                margin: UiRect::top(px(6.0)),
                padding: UiRect::all(px(6.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(5.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(0.30, 0.36, 0.40, 0.50)),
            SidebarPageContent::page(SidebarPage::Loads),
            Name::new("CreateFromSelectionPanel"),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("Create from selection"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(Color::srgba(0.44, 0.60, 0.72, 0.90)),
            ));

            panel
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4.0),
                    ..default()
                },))
                .with_children(|row| {
                    constraint_preset_button(row, "Fix All", 1, 6);
                    constraint_preset_button(row, "Fix XYZ", 1, 3);
                    constraint_preset_button(row, "Fix X", 1, 1);
                    constraint_preset_button(row, "Fix Y", 2, 2);
                    constraint_preset_button(row, "Fix Z", 3, 3);
                });
            spawn_constraint_exact_editor(panel);

            spawn_nodal_exact_editor(panel);
            panel
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4.0),
                    ..default()
                },))
                .with_children(|row| {
                    load_direction_button(row, "+X", 1, 1.0);
                    load_direction_button(row, "-X", 1, -1.0);
                    load_direction_button(row, "+Y", 2, 1.0);
                    load_direction_button(row, "-Y", 2, -1.0);
                    load_direction_button(row, "+Z", 3, 1.0);
                    load_direction_button(row, "-Z", 3, -1.0);
                });
            panel
                .spawn((
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        min_height: px(24.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(4.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(PANEL_BORDER),
                    LoadDirectionPickerButton,
                    Name::new("LoadDirectionPickerButton"),
                ))
                .with_child((
                    Text::new("Pick direction in viewport - select nodes first"),
                    TextFont {
                        font_size: FontSize::Px(9.5),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                    LoadDirectionPickerLabel,
                ));
            spawn_slider(
                panel,
                SliderConfig {
                    width: 268.0,
                    min: 0.0,
                    max: 1000.0,
                    value: 100.0,
                    label: "Load magnitude",
                    id: SliderId::LoadMagnitude,
                },
            );
            hint_text(
                panel,
                "Axis button or viewport arrow -> preview; exact value at lower right",
            );
            panel
                .spawn((
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        height: px(24.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(4.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(PANEL_BORDER),
                    ApplyLoadButton,
                    Name::new("ApplyLoadButton"),
                ))
                .with_child((
                    Text::new("Apply Load to Selection"),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                    ApplyLoadLabel,
                ));

            panel
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4.0),
                        margin: UiRect::top(px(6.0)),
                        padding: UiRect::all(px(6.0)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(5.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgba(0.40, 0.36, 0.20, 0.50)),
                    Name::new("DloadPanel"),
                ))
                .with_children(|distributed| {
                    distributed.spawn((
                        Text::new("Add Distributed Load (select faces)"),
                        TextFont {
                            font_size: FontSize::Px(9.5),
                            ..default()
                        },
                        TextColor(Color::srgba(0.74, 0.68, 0.40, 0.90)),
                    ));

                    distributed
                        .spawn((Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(4.0),
                            ..default()
                        },))
                        .with_children(|row| {
                            for (kind, label) in [
                                (SelectedDloadKind::Pressure, "Pressure"),
                                (SelectedDloadKind::Gravity, "Gravity"),
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
                                    BorderColor::all(PANEL_BORDER),
                                    DloadKindButton(kind),
                                    Name::new(format!("DloadKind_{label}")),
                                ))
                                .with_child((
                                    Text::new(label),
                                    TextFont {
                                        font_size: FontSize::Px(9.5),
                                        ..default()
                                    },
                                    TextColor(TEXT_MAIN),
                                ));
                            }
                        });

                    spawn_dload_exact_editor(distributed);
                    spawn_slider(
                        distributed,
                        SliderConfig {
                            width: 268.0,
                            min: 0.0,
                            max: 100.0,
                            value: 1.0,
                            label: "Pressure / Accel. magnitude",
                            id: SliderId::DloadMagnitude,
                        },
                    );
                    distributed
                        .spawn((
                            Button,
                            Node {
                                width: Val::Percent(100.0),
                                height: px(24.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            ApplyDloadButton,
                            Name::new("ApplyDloadButton"),
                        ))
                        .with_child((
                            Text::new("Apply Distributed Load"),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                            ApplyDloadLabel,
                        ));
                    hint_text(
                        distributed,
                        "Pressure: select faces  Gravity: select elements  Apply commits",
                    );
                });

            panel
                .spawn((
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        height: px(22.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(4.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.40, 0.12, 0.12, 0.80)),
                    BorderColor::all(Color::srgba(0.65, 0.20, 0.20, 0.80)),
                    ClearAllBcLoadsButton,
                    Name::new("ClearAllBcLoadsButton"),
                ))
                .with_child((
                    Text::new("Clear All BCs & Loads"),
                    TextFont {
                        font_size: FontSize::Px(9.5),
                        ..default()
                    },
                    TextColor(Color::srgb(0.95, 0.80, 0.80)),
                ));
            hint_text(
                panel,
                "Viewport controls set a draft; exact fields and Apply are authoritative",
            );
            spawn_engineering_input_status(panel);
        });
}

fn action_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &'static str,
    marker: M,
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
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));
}

fn constraint_preset_button(
    parent: &mut ChildSpawnerCommands,
    label: &'static str,
    dof_start: u8,
    dof_end: u8,
) {
    parent
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                height: px(24.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            ConstraintPresetButton {
                dof_start,
                dof_end,
                label,
            },
            Name::new(format!("ConstraintPreset_{label}")),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(9.5),
                ..default()
            },
            TextColor(TEXT_MAIN),
            ConstraintPresetLabel,
        ));
}

fn load_direction_button(
    parent: &mut ChildSpawnerCommands,
    label: &'static str,
    dof: u8,
    sign: f32,
) {
    parent
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                height: px(24.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            LoadDirectionButton { dof, sign },
            Name::new(format!("LoadDirection_{label}")),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(9.5),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));
}

fn hint_text(parent: &mut ChildSpawnerCommands, text: &'static str) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(9.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
    ));
}
pub(crate) fn rebuild_boundary_loads_list(
    mut commands: Commands,
    setup: Res<fem_core::AnalysisSetup>,
    container_query: Query<Entity, With<BoundaryLoadsListContainer>>,
    children_query: Query<&Children>,
) {
    if !setup.is_changed() {
        return;
    }

    let Ok(container) = container_query.single() else {
        return;
    };

    if let Ok(children) = children_query.get(container) {
        for &child in children {
            commands.entity(child).despawn();
        }
    }

    commands.entity(container).with_children(|list| {
        for (index, bc) in setup.boundary_conditions.iter().enumerate() {
            let center_label = bc
                .rotation_center
                .as_ref()
                .map(|center| {
                    center
                        .ngrp_name
                        .clone()
                        .or_else(|| center.node.map(|node| node.0.to_string()))
                        .map(|token| format!(" about {token}"))
                        .unwrap_or_else(|| " about unresolved center".to_string())
                })
                .unwrap_or_default();
            let label = format!(
                "[BC] {}  {}{}  ({} nodes)  val={:.4}",
                bc.name,
                bc.dof_label(),
                center_label,
                bc.nodes.len(),
                bc.value
            );
            setup_entry_row(
                list,
                &label,
                DeleteSetupEntry::BoundaryCondition(index),
                &format!("BC_{}", bc.name),
            );
        }

        // Group nodal loads by name for display (one entry per unique name).
        let mut seen_load_names: Vec<&str> = Vec::new();
        for (index, load) in setup.nodal_loads.iter().enumerate() {
            if seen_load_names.contains(&load.name.as_str()) {
                continue;
            }
            seen_load_names.push(&load.name);
            let group: Vec<_> = setup
                .nodal_loads
                .iter()
                .filter(|l| l.name == load.name)
                .collect();
            let components: BTreeSet<_> = group.iter().map(|load| load.dof_label()).collect();
            let nodes: BTreeSet<_> = group
                .iter()
                .map(|load| (load.mesh_index, load.node))
                .collect();
            let center_label = group
                .iter()
                .find_map(|load| load.rotation_center.as_ref())
                .map(|center| {
                    center
                        .ngrp_name
                        .clone()
                        .or_else(|| center.node.map(|node| node.0.to_string()))
                        .map(|token| format!(" about {token}"))
                        .unwrap_or_else(|| " about unresolved center".to_string())
                })
                .unwrap_or_default();
            let label = format!(
                "[Load] {}  {}{}  ({} nodes)",
                load.name,
                components.into_iter().collect::<Vec<_>>().join("/"),
                center_label,
                nodes.len()
            );
            setup_entry_row(
                list,
                &label,
                DeleteSetupEntry::LoadGroup(index),
                &format!("Load_{}", load.name),
            );
        }

        for (index, dload) in setup.distributed_loads.iter().enumerate() {
            let kind_label = match dload.kind {
                fem_core::DistributedLoadKind::Pressure => "Pressure",
                fem_core::DistributedLoadKind::Gravity => "Gravity",
            };
            let unit = match dload.kind {
                fem_core::DistributedLoadKind::Pressure => "faces",
                fem_core::DistributedLoadKind::Gravity => "elems",
            };
            let label = format!(
                "[DLoad] {}  {kind_label}={:.3}  ({} {unit})",
                dload.name,
                dload.value,
                dload.target.len()
            );
            setup_entry_row(
                list,
                &label,
                DeleteSetupEntry::DistributedLoad(index),
                &format!("DLoad_{}", dload.name),
            );
        }

        if setup.boundary_conditions.is_empty()
            && setup.nodal_loads.is_empty()
            && setup.distributed_loads.is_empty()
        {
            list.spawn((
                Text::new("(none yet - select nodes and use buttons above)"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
        }
    });
}

/// Applies a constraint preset to all currently selected nodes.
///
/// One [`BoundaryCondition`] entry is created per contiguous call, named
/// `BC1`, `BC2`, … by [`AnalysisSetup::next_auto_name`]. Using a separate
/// entry per click (rather than merging into an existing one) keeps the
/// list simple and undo straightforward: delete the most-recent entry to
/// revert the action.
// ── surface selection growth ─────────────────────────────────────────────────

pub(crate) fn update_constraint_button_labels(
    selection: Res<SelectionState>,
    buttons: Query<(&ConstraintPresetButton, &Children), Without<ConstraintPresetLabel>>,
    mut labels: Query<&mut Text, With<ConstraintPresetLabel>>,
) {
    if !selection.is_changed() {
        return;
    }

    let n = selection
        .targets
        .iter()
        .filter(|t| matches!(t.entity, fem_core::FemEntityId::Node(_)))
        .count();

    for (btn, children) in &buttons {
        for &child in children {
            if let Ok(mut text) = labels.get_mut(child) {
                **text = if n > 0 {
                    format!("{} ({})", btn.label, n)
                } else {
                    btn.label.to_string()
                };
            }
        }
    }
}

/// Updates the "Apply Load" button label with the node count.
pub(crate) fn update_apply_load_label(
    selection: Res<SelectionState>,
    editor: Res<BoundaryLoadEditorState>,
    mut labels: Query<&mut Text, With<ApplyLoadLabel>>,
) {
    if !selection.is_changed() && !editor.is_changed() {
        return;
    }

    let Ok(mut text) = labels.single_mut() else {
        return;
    };

    let n = selection
        .targets
        .iter()
        .filter(|t| matches!(t.entity, fem_core::FemEntityId::Node(_)))
        .count();

    let component_count = editor
        .nodal_components
        .iter()
        .filter(|value| value.abs() > f32::EPSILON)
        .count();

    let needs_center = editor.load_moment_mode == RotationalInputMode::AboutCenter
        && editor.nodal_components[3..]
            .iter()
            .any(|value| value.abs() > f32::EPSILON)
        && editor.rotation_center.is_none();

    **text = if needs_center {
        "Apply Load - set one rotation center".to_string()
    } else if n > 0 {
        format!("Apply {component_count} load components  ({n} nodes)")
    } else {
        "Apply Load - no nodes selected".to_string()
    };
}

/// Clears all boundary conditions and nodal loads at once.
pub(crate) fn clear_all_bc_loads_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut buttons: Query<(Ref<Interaction>, &mut BackgroundColor), With<ClearAllBcLoadsButton>>,
) {
    for (interaction, mut bg) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            setup.boundary_conditions.clear();
            setup.nodal_loads.clear();
            setup.distributed_loads.clear();
            setup.set_changed();
        }

        *bg = BackgroundColor(match *interaction {
            Interaction::Pressed | Interaction::Hovered => Color::srgba(0.60, 0.15, 0.15, 0.95),
            Interaction::None => Color::srgba(0.40, 0.12, 0.12, 0.80),
        });
    }
}

pub(crate) fn selected_nodes_by_mesh(
    selection: &SelectionState,
) -> BTreeMap<usize, Vec<fem_core::NodeId>> {
    let mut by_mesh = BTreeMap::<usize, BTreeSet<fem_core::NodeId>>::new();

    for target in &selection.targets {
        if let FemEntityId::Node(id) = target.entity {
            by_mesh.entry(target.mesh_index).or_default().insert(id);
        }
    }

    by_mesh
        .into_iter()
        .map(|(mesh_index, nodes)| (mesh_index, nodes.into_iter().collect()))
        .collect()
}

pub(crate) fn constraint_preset_button_system(
    mut editor: ResMut<BoundaryLoadEditorState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &ConstraintPresetButton,
        ),
        With<ConstraintPresetButton>,
    >,
) {
    for (interaction, mut bg, mut border, preset) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            editor.set_constraint_preset(preset.dof_start, preset.dof_end);
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

/// Toggles the load direction selection; tracks active direction in
/// [`SelectedLoadDirection`] and highlights the active button.
pub(crate) fn load_direction_button_system(
    mut selected: ResMut<SelectedLoadDirection>,
    mut active_editor: ResMut<ActiveLoadEditor>,
    mut editor: ResMut<BoundaryLoadEditorState>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &LoadDirectionButton,
        ),
        With<LoadDirectionButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let new_dir = (btn.dof, btn.sign);
            if selected.0 == Some(new_dir) {
                selected.0 = None; // toggle off
                *active_editor = ActiveLoadEditor::None;
                measurement.clear();
            } else {
                selected.0 = Some(new_dir);
                *active_editor = ActiveLoadEditor::Nodal;
                let magnitude = slider_value(&sliders, SliderId::LoadMagnitude, 100.0);
                editor.set_axis_force(btn.dof, btn.sign, magnitude);
                measurement.begin_slider_value(
                    SliderId::LoadMagnitude,
                    nodal_load_measurement_label(btn.dof, btn.sign),
                    "analysis force units",
                    magnitude,
                );
            }
        }

        let active = selected.0 == Some((btn.dof, btn.sign));
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };

        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

fn nodal_load_measurement_label(dof: u8, sign: f32) -> &'static str {
    match (dof, sign >= 0.0) {
        (1, true) => "Nodal load +X",
        (1, false) => "Nodal load -X",
        (2, true) => "Nodal load +Y",
        (2, false) => "Nodal load -Y",
        (3, true) => "Nodal load +Z",
        (3, false) => "Nodal load -Z",
        _ => "Nodal load",
    }
}

fn slider_value(
    sliders: &Query<&SliderState, With<SliderTrack>>,
    id: SliderId,
    fallback: f32,
) -> f32 {
    sliders
        .iter()
        .find(|slider| slider.id == id)
        .map(|slider| slider.value)
        .unwrap_or(fallback)
}

/// Applies every non-zero exact force / moment component to each selected
/// node. Axis buttons, the viewport compass, and the slider only populate
/// this same draft; they are never a second source of solver values.
pub(crate) fn apply_load_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    model: Option<Res<FemModel>>,
    selection: Res<SelectionState>,
    editor: Res<BoundaryLoadEditorState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ApplyLoadButton>,
    >,
) {
    let Some(model) = model else {
        return;
    };

    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let needs_center = editor.load_moment_mode == RotationalInputMode::AboutCenter
                && editor.nodal_components[3..]
                    .iter()
                    .any(|value| value.abs() > f32::EPSILON);
            if needs_center && editor.resolved_rotation_center().is_none() {
                continue;
            }
            let components: Vec<_> = editor
                .nodal_components
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, value)| value.is_finite() && value.abs() > f32::EPSILON)
                .map(|(index, value)| (index, value))
                .collect();
            if components.is_empty() {
                continue;
            }

            let mut added = false;
            for (mesh_index, nodes) in selected_nodes_by_mesh(&selection) {
                if nodes.is_empty() || model.meshes.get(mesh_index).is_none() {
                    continue;
                }

                let name = setup.next_auto_name_pub("LOAD");

                for &(index, value) in &components {
                    let Some((dof, rotation_center)) = editor.load_solver_component(index) else {
                        continue;
                    };
                    for &node in &nodes {
                        setup.nodal_loads.push(fem_core::NodalLoad {
                            name: name.clone(),
                            mesh_index,
                            node,
                            ngrp_name: None,
                            rotation_center: rotation_center.clone(),
                            dof,
                            value,
                        });
                        added = true;
                    }
                }
            }
            if added {
                setup.set_changed();
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

/// Adds one of the built-in material presets to [`AnalysisSetup`]. If a
/// material with the same name already exists the button is a no-op (to
/// avoid duplicate entries cluttering the list).
/// Toggles the active [`SelectedDloadKind`] when [Pressure]/[Gravity] clicked.
pub(crate) fn dload_kind_button_system(
    mut selected: ResMut<SelectedDloadKind>,
    mut active_editor: ResMut<ActiveLoadEditor>,
    editor: Res<BoundaryLoadEditorState>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &DloadKindButton,
        ),
        With<DloadKindButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            *selected = btn.0;
            *active_editor = ActiveLoadEditor::Distributed;
            let (label, units, value) = match btn.0 {
                SelectedDloadKind::Pressure => {
                    ("Pressure", "analysis pressure units", editor.pressure)
                }
                SelectedDloadKind::Gravity => (
                    "Gravity acceleration",
                    "analysis accel. units",
                    editor.gravity_acceleration,
                ),
            };
            measurement.begin_slider_value(SliderId::DloadMagnitude, label, units, value);
        }

        let active = *selected == btn.0;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Keeps the shared lower-right numeric field attached to whichever load
/// authoring control was used most recently.
pub(crate) fn sync_load_measurement_box(
    page: Res<SidebarPage>,
    active_editor: Res<ActiveLoadEditor>,
    selected_direction: Res<SelectedLoadDirection>,
    kind: Res<SelectedDloadKind>,
    editor: Res<BoundaryLoadEditorState>,
    mut measurement: ResMut<MeasurementBoxState>,
) {
    if *page != SidebarPage::Loads {
        return;
    }

    let (slider_id, label, units, value) = match *active_editor {
        ActiveLoadEditor::None => return,
        ActiveLoadEditor::Nodal => {
            let Some((dof, sign)) = selected_direction.0 else {
                return;
            };
            (
                SliderId::LoadMagnitude,
                nodal_load_measurement_label(dof, sign),
                "analysis force units",
                editor.nodal_components[usize::from(dof.saturating_sub(1))].abs(),
            )
        }
        ActiveLoadEditor::Distributed => match *kind {
            SelectedDloadKind::Pressure => (
                SliderId::DloadMagnitude,
                "Pressure",
                "analysis pressure units",
                editor.pressure,
            ),
            SelectedDloadKind::Gravity => (
                SliderId::DloadMagnitude,
                "Gravity acceleration",
                "analysis accel. units",
                editor.gravity_acceleration,
            ),
        },
    };

    let target_matches = matches!(
        measurement.target,
        Some(MeasurementTarget::SliderValue {
            slider_id: target,
            ..
        }) if target == slider_id
    );

    if !target_matches {
        measurement.begin_slider_value(slider_id, label, units, value);
    } else if editor.is_changed() {
        measurement.update_slider_value(slider_id, value);
    }
}

/// Builds provisional load arrows from the live selection. The resulting
/// resource is view-only; Apply remains the explicit commit boundary.
pub(crate) fn update_boundary_load_preview(
    page: Res<SidebarPage>,
    active_editor: Res<ActiveLoadEditor>,
    kind: Res<SelectedDloadKind>,
    editor: Res<BoundaryLoadEditorState>,
    selection: Res<SelectionState>,
    model: Option<Res<FemModel>>,
    mut preview: ResMut<BoundaryLoadPreview>,
) {
    let model_changed = model.as_ref().is_some_and(|model| model.is_changed());
    if !page.is_changed()
        && !active_editor.is_changed()
        && !kind.is_changed()
        && !editor.is_changed()
        && !selection.is_changed()
        && !model_changed
    {
        return;
    }

    let Some(model) = model.as_deref() else {
        if *preview != BoundaryLoadPreview::default() {
            *preview = BoundaryLoadPreview::default();
        }
        return;
    };
    if *page != SidebarPage::Loads {
        if *preview != BoundaryLoadPreview::default() {
            *preview = BoundaryLoadPreview::default();
        }
        return;
    }

    let mut next = BoundaryLoadPreview::default();

    match *active_editor {
        ActiveLoadEditor::None => {}
        ActiveLoadEditor::Nodal => {
            let force_direction = editor.translational_force().try_normalize();
            let moment_axis = editor.rotational_moment().try_normalize();
            if force_direction.is_none() && moment_axis.is_none() {
                if *preview != next {
                    *preview = next;
                }
                return;
            }
            next.kind = Some(BoundaryLoadPreviewKind::Nodal);
            for target in &selection.targets {
                let FemEntityId::Node(node_id) = target.entity else {
                    continue;
                };
                let Some(position) = model
                    .meshes
                    .get(target.mesh_index)
                    .and_then(|mesh| mesh.node_position(node_id))
                else {
                    continue;
                };
                if let Some(direction) = force_direction {
                    next.arrows.push(BoundaryLoadPreviewArrow {
                        origin: position,
                        direction,
                    });
                }
                if let Some(axis) = moment_axis {
                    if editor.load_moment_mode == RotationalInputMode::DirectDof {
                        next.moments.push(BoundaryLoadPreviewMoment {
                            origin: position,
                            axis,
                        });
                    }
                }
            }
            if let (Some(axis), Some(center)) = (moment_axis, editor.rotation_center) {
                if editor.load_moment_mode == RotationalInputMode::AboutCenter {
                    if let FemEntityId::Node(node) = center.entity {
                        if let Some(origin) = model
                            .meshes
                            .get(center.mesh_index)
                            .and_then(|mesh| mesh.node_position(node))
                        {
                            next.moments
                                .push(BoundaryLoadPreviewMoment { origin, axis });
                        }
                    }
                }
            }
        }
        ActiveLoadEditor::Distributed => match *kind {
            SelectedDloadKind::Pressure => {
                let magnitude = editor.pressure;
                next.kind = Some(BoundaryLoadPreviewKind::Pressure);
                for (mesh_index, face_refs) in
                    selected_faces_from_faces_or_elements(&selection, model)
                {
                    let Some(mesh) = model.meshes.get(mesh_index) else {
                        continue;
                    };
                    let selected: BTreeSet<_> = face_refs.into_iter().collect();
                    next.arrows.extend(
                        mesh.cached_boundary_faces()
                            .iter()
                            .filter(|face| {
                                face.element_face_ref()
                                    .is_some_and(|face_ref| selected.contains(&face_ref))
                            })
                            .filter_map(|face| mesh.face_geometry(face))
                            .map(|geometry| BoundaryLoadPreviewArrow {
                                origin: geometry.centroid,
                                direction: signed_preview_direction(-geometry.normal, magnitude),
                            }),
                    );
                }
            }
            SelectedDloadKind::Gravity => {
                let magnitude = editor.gravity_acceleration;
                let direction = editor.normalized_gravity_direction().unwrap_or(Vec3::ZERO);
                next.kind = Some(BoundaryLoadPreviewKind::Gravity);
                for (mesh_index, element_ids) in
                    selected_elements_from_faces_or_elements(&selection, model)
                {
                    let Some(mesh) = model.meshes.get(mesh_index) else {
                        continue;
                    };
                    let selected: BTreeSet<_> = element_ids.into_iter().collect();
                    let mut centroid = Vec3::ZERO;
                    let mut count = 0usize;
                    for element in &mesh.elements {
                        if !selected.contains(&element.id) {
                            continue;
                        }
                        if let Some(positions) = mesh.node_positions(&element.nodes) {
                            for position in positions {
                                centroid += position;
                                count += 1;
                            }
                        }
                    }
                    if count > 0 {
                        next.arrows.push(BoundaryLoadPreviewArrow {
                            origin: centroid / count as f32,
                            direction: signed_preview_direction(direction, magnitude),
                        });
                    }
                }
            }
        },
    }

    if *preview != next {
        *preview = next;
    }
}

pub(crate) fn signed_preview_direction(direction: Vec3, magnitude: f32) -> Vec3 {
    if magnitude.abs() <= f32::EPSILON {
        Vec3::ZERO
    } else if magnitude < 0.0 {
        -direction
    } else {
        direction
    }
}

/// Resolves the parent element of every currently selected face.
///
/// Faces selected via the Face filter carry an `element: Option<ElementId>`
/// back-reference set when boundary faces were cached; this just gathers
/// the unique non-`None` values. If the person has Element filter active and
/// selected elements directly instead, those are used as a fallback so the
/// Apply button still does something sensible regardless of filter mode.
fn selected_elements_from_faces_or_elements(
    selection: &SelectionState,
    model: &FemModel,
) -> BTreeMap<usize, Vec<fem_core::ElementId>> {
    let mut by_mesh = BTreeMap::<usize, BTreeSet<fem_core::ElementId>>::new();

    for target in &selection.targets {
        let Some(mesh) = model.meshes.get(target.mesh_index) else {
            continue;
        };

        let element = match target.entity {
            FemEntityId::Face(id) => mesh
                .cached_boundary_faces()
                .iter()
                .find(|face| face.id == id)
                .and_then(|face| face.element),
            FemEntityId::Element(id) => Some(id),
            FemEntityId::Node(_) | FemEntityId::Edge(_) => None,
        };

        if let Some(element) = element {
            by_mesh
                .entry(target.mesh_index)
                .or_default()
                .insert(element);
        }
    }

    by_mesh
        .into_iter()
        .map(|(mesh_index, elements)| (mesh_index, elements.into_iter().collect()))
        .collect()
}

/// Resolves the current selection to boundary element-faces (element +
/// local face index), for pressure [`fem_core::DistributedLoad`]s — unlike
/// [`selected_elements_from_faces_or_elements`], this keeps which specific
/// local face was picked, since FrontISTR's pressure `!DLOAD` needs that
/// (`P1`..`P6`) per element.
///
/// Delegates to [`fem_core::FemMesh::surface_refs_from_targets`] (the same
/// resolution `create_surface_button_system` uses for contact surface
/// sets): a `Face` target maps directly to its owning element's face, and
/// an `Element` target expands to every boundary face of that element.
///
fn selected_faces_from_faces_or_elements(
    selection: &SelectionState,
    model: &FemModel,
) -> BTreeMap<usize, Vec<fem_core::ElementFaceRef>> {
    let mut by_mesh = BTreeMap::<usize, BTreeSet<fem_core::ElementFaceRef>>::new();

    for target in &selection.targets {
        let Some(mesh) = model.meshes.get(target.mesh_index) else {
            continue;
        };

        for face_ref in mesh.surface_refs_from_targets(&[target.entity]) {
            by_mesh
                .entry(target.mesh_index)
                .or_default()
                .insert(face_ref);
        }
    }

    by_mesh
        .into_iter()
        .map(|(mesh_index, faces)| (mesh_index, faces.into_iter().collect()))
        .collect()
}

/// Updates the [`ApplyDloadButton`]'s label with the current face/element
/// count, mirroring [`update_apply_load_label`]'s feedback pattern.
pub(crate) fn update_apply_dload_label(
    selection: Res<SelectionState>,
    model: Option<Res<FemModel>>,
    kind: Res<SelectedDloadKind>,
    editor: Res<BoundaryLoadEditorState>,
    mut labels: Query<&mut Text, With<ApplyDloadLabel>>,
) {
    if !selection.is_changed() && !kind.is_changed() && !editor.is_changed() {
        return;
    }

    let Ok(mut text) = labels.single_mut() else {
        return;
    };
    let Some(model) = model.as_deref() else {
        return;
    };

    // Pressure counts picked *faces* (what actually gets written to the
    // .cnt); gravity counts elements, since it has no face to speak of.
    let (n, unit) = match *kind {
        SelectedDloadKind::Pressure => (
            selected_faces_from_faces_or_elements(&selection, model)
                .values()
                .map(Vec::len)
                .sum::<usize>(),
            "faces",
        ),
        SelectedDloadKind::Gravity => (
            selected_elements_from_faces_or_elements(&selection, model)
                .values()
                .map(Vec::len)
                .sum::<usize>(),
            "elements",
        ),
    };

    let mag = match *kind {
        SelectedDloadKind::Pressure => editor.pressure,
        SelectedDloadKind::Gravity => editor.gravity_acceleration,
    };

    let kind_label = match *kind {
        SelectedDloadKind::Pressure => "Pressure",
        SelectedDloadKind::Gravity => "Gravity",
    };

    **text =
        if *kind == SelectedDloadKind::Gravity && editor.normalized_gravity_direction().is_none() {
            "Apply Gravity - direction must be non-zero".to_string()
        } else if n > 0 {
            format!("Apply {kind_label} {mag:.2}  ({n} {unit})")
        } else {
            format!("Apply {kind_label}  - no faces/elements selected")
        };
}

/// Creates a [`fem_core::DistributedLoad`] from the currently selected faces
/// (resolved to their parent elements) and the configured kind/magnitude.
pub(crate) fn apply_dload_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    model: Option<Res<FemModel>>,
    selection: Res<SelectionState>,
    kind: Res<SelectedDloadKind>,
    editor: Res<BoundaryLoadEditorState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ApplyDloadButton>,
    >,
) {
    let Some(model) = model.as_deref() else {
        return;
    };

    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let mut added = false;
            // Pressure needs which face was picked (P1..P6 in the exported
            // .cnt); gravity is a whole-element body force and has no face.
            match *kind {
                SelectedDloadKind::Pressure => {
                    for (mesh_index, faces) in
                        selected_faces_from_faces_or_elements(&selection, model)
                    {
                        if faces.is_empty() {
                            continue;
                        }

                        let name = setup.next_auto_name_pub("DLOAD");
                        setup.distributed_loads.push(fem_core::DistributedLoad {
                            name,
                            mesh_index,
                            target: fem_core::DistributedLoadTarget::Faces(faces),
                            kind: fem_core::DistributedLoadKind::Pressure,
                            value: editor.pressure,
                            direction: None,
                        });
                        added = true;
                    }
                }
                SelectedDloadKind::Gravity => {
                    let Some(direction) = editor.normalized_gravity_direction() else {
                        continue;
                    };
                    for (mesh_index, elements) in
                        selected_elements_from_faces_or_elements(&selection, model)
                    {
                        if elements.is_empty() {
                            continue;
                        }

                        let name = setup.next_auto_name_pub("DLOAD");
                        setup.distributed_loads.push(fem_core::DistributedLoad {
                            name,
                            mesh_index,
                            target: fem_core::DistributedLoadTarget::Elements(elements),
                            kind: fem_core::DistributedLoadKind::Gravity,
                            value: editor.gravity_acceleration,
                            direction: Some(direction),
                        });
                        added = true;
                    }
                }
            }

            if added {
                setup.set_changed();
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
pub(crate) fn toggle_constraints_button_system(
    mut settings: ResMut<visualization::BoundaryVisualSettings>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ToggleConstraintsButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            settings.show_constraints = !settings.show_constraints;
        }

        let active = settings.show_constraints;

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

/// Toggles [`visualization::BoundaryVisualSettings::show_loads`].
pub(crate) fn toggle_loads_button_system(
    mut settings: ResMut<visualization::BoundaryVisualSettings>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ToggleLoadsButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            settings.show_loads = !settings.show_loads;
        }

        let active = settings.show_loads;

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
