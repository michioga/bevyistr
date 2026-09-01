use crate::layout::{DeleteSetupEntry, ScrollableList, SidebarPage, SidebarPageContent};
use crate::slider::{SliderConfig, SliderId, SliderState, SliderTrack, spawn_slider};
use bevy::prelude::*;
use bevy::ui::ScrollPosition;
use fem_core::{FemModel, FemModelVersion};

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SelectedSectionType {
    #[default]
    Solid,
    Shell,
    Beam,
}

#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct SelectedEgrp(pub Option<String>);

#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct SelectedMaterialForSection(pub Option<String>);

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SectionTypeButton(pub SelectedSectionType);

#[derive(Component, Debug, Clone)]
pub(crate) struct EgrpSelectButton(pub Option<String>);

#[derive(Component, Debug, Clone)]
pub(crate) struct MaterialSelectButton(pub String);

#[derive(Component)]
pub(crate) struct AddSectionButton;

#[derive(Component)]
pub(crate) struct SectionDefEgrpRow;

#[derive(Component)]
pub(crate) struct SectionDefMatRow;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct MaterialPresetButton {
    pub preset_index: usize,
}

#[derive(Component)]
pub(crate) struct MaterialsSectionsListContainer;

pub(crate) fn spawn_materials_ui(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(5.0),
                padding: UiRect::all(px(6.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(5.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(0.30, 0.50, 0.36, 0.50)),
            SidebarPageContent::page(SidebarPage::Materials),
            Name::new("MaterialsEditorPanel"),
        ))
        .with_children(|panel| {
            panel
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4.0),
                    margin: UiRect::top(px(4.0)),
                    ..default()
                },))
                .with_children(|row| {
                    for (index, preset) in material_presets().iter().enumerate() {
                        material_preset_button(row, index, preset.label);
                    }
                });

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
                    BorderColor::all(Color::srgba(0.30, 0.50, 0.36, 0.50)),
                    Name::new("SectionDefPanel"),
                ))
                .with_children(|section| {
                    section.spawn((
                        Text::new("Add Section"),
                        TextFont {
                            font_size: FontSize::Px(9.5),
                            ..default()
                        },
                        TextColor(Color::srgba(0.44, 0.70, 0.54, 0.90)),
                    ));

                    section
                        .spawn((Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(4.0),
                            ..default()
                        },))
                        .with_children(|row| {
                            for (kind, label) in [
                                (SelectedSectionType::Solid, "Solid"),
                                (SelectedSectionType::Shell, "Shell"),
                                (SelectedSectionType::Beam, "Beam"),
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
                                    SectionTypeButton(kind),
                                    Name::new(format!("SectionType_{label}")),
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

                    spawn_slider(
                        section,
                        SliderConfig {
                            width: 268.0,
                            min: 0.0,
                            max: 50.0,
                            value: 2.0,
                            label: "Thickness / Area",
                            id: SliderId::SectionThickness,
                        },
                    );

                    section.spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(4.0),
                            flex_wrap: FlexWrap::Wrap,
                            row_gap: px(4.0),
                            ..default()
                        },
                        SectionDefEgrpRow,
                        Name::new("SectionDefEgrpRow"),
                    ));
                    section.spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(4.0),
                            flex_wrap: FlexWrap::Wrap,
                            row_gap: px(4.0),
                            ..default()
                        },
                        SectionDefMatRow,
                        Name::new("SectionDefMatRow"),
                    ));
                    section
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
                            AddSectionButton,
                            Name::new("AddSectionButton"),
                        ))
                        .with_child((
                            Text::new("Add Section"),
                            TextFont {
                                font_size: FontSize::Px(10.5),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                });
        });

    parent.spawn((
        Text::new("Materials & sections:"),
        TextFont {
            font_size: FontSize::Px(9.5),
            ..default()
        },
        TextColor(Color::srgba(0.44, 0.60, 0.72, 0.90)),
        Node {
            margin: UiRect::top(px(4.0)),
            ..default()
        },
        SidebarPageContent::page(SidebarPage::Materials),
    ));
    parent.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3.0),
            max_height: px(90.0),
            overflow: Overflow::scroll_y(),
            margin: UiRect::top(px(4.0)),
            ..default()
        },
        ScrollPosition::default(),
        ScrollableList,
        MaterialsSectionsListContainer,
        SidebarPageContent::page(SidebarPage::Materials),
        Name::new("MaterialsSectionsListContainer"),
    ));
}

struct MaterialPreset {
    label: &'static str,
    name: &'static str,
    young_modulus: f32,
    poisson_ratio: f32,
    density: f32,
}

fn material_presets() -> &'static [MaterialPreset] {
    const PRESETS: &[MaterialPreset] = &[
        MaterialPreset {
            label: "+ Steel",
            name: "STEEL",
            young_modulus: 2.05e11,
            poisson_ratio: 0.30,
            density: 7850.0,
        },
        MaterialPreset {
            label: "+ Aluminum",
            name: "ALUMINUM",
            young_modulus: 6.90e10,
            poisson_ratio: 0.33,
            density: 2700.0,
        },
        MaterialPreset {
            label: "+ Concrete",
            name: "CONCRETE",
            young_modulus: 3.00e10,
            poisson_ratio: 0.20,
            density: 2400.0,
        },
        MaterialPreset {
            label: "+ Titanium",
            name: "TITANIUM",
            young_modulus: 1.14e11,
            poisson_ratio: 0.34,
            density: 4500.0,
        },
    ];

    PRESETS
}

fn material_preset_button(
    parent: &mut ChildSpawnerCommands,
    preset_index: usize,
    label: &'static str,
) {
    parent
        .spawn((
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
            MaterialPresetButton { preset_index },
            Name::new(format!("MaterialPreset_{label}")),
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
pub(crate) fn material_preset_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &MaterialPresetButton,
        ),
        With<MaterialPresetButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(preset) = material_presets().get(btn.preset_index) {
                if setup.material_by_name(preset.name).is_none() {
                    setup.add_material(
                        preset.name,
                        Some(preset.young_modulus),
                        Some(preset.poisson_ratio),
                        Some(preset.density),
                    );
                }
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

/// Handles clicks on the ✕ delete buttons in the BC/load and
/// material/section lists, removing the corresponding entry from
/// [`AnalysisSetup`]. Changes trigger `is_changed()` on the resource so
/// the list-rebuild systems re-run automatically and the 3D boundary
/// visuals respawn.
pub(crate) fn rebuild_materials_sections_list(
    mut commands: Commands,
    setup: Res<fem_core::AnalysisSetup>,
    container_query: Query<Entity, With<MaterialsSectionsListContainer>>,
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
        for (index, material) in setup.materials.iter().enumerate() {
            setup_entry_row(
                list,
                &format_material_line(material),
                DeleteSetupEntry::Material(index),
                &format!("MaterialLine_{}", material.name),
            );
        }

        for (index, section) in setup.sections.iter().enumerate() {
            setup_entry_row(
                list,
                &format_section_line(section),
                DeleteSetupEntry::Section(index),
                &format!("SectionLine_{}", section.name),
            );
        }
    });
}

/// Spawns one removable list entry: a text line plus a small "✕" button
/// tagged with [`DeleteSetupEntry`], used by both
/// [`rebuild_materials_sections_list`] and (for boundary
/// conditions/loads) the constraint/load panels.
pub(crate) fn setup_entry_row(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    delete_entry: DeleteSetupEntry,
    name: &str,
) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6.0),
                ..default()
            },
            Name::new(name.to_string()),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(label.to_string()),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            row.spawn((
                Button,
                Node {
                    width: px(16.0),
                    height: px(16.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(px(3.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.55, 0.18, 0.18, 0.85)),
                delete_entry,
                Name::new(format!("{name}_DeleteButton")),
            ))
            .with_child((
                Text::new("x"),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(Color::srgb(0.95, 0.85, 0.85)),
            ));
        });
}

/// Formats one [`fem_core::FemMaterial`] as `"[Mat] name  E=.. nu=.. rho=.."`,
/// omitting any property that wasn't present in the source file rather
/// than showing a misleading placeholder value.
fn format_material_line(material: &fem_core::FemMaterial) -> String {
    let mut parts = Vec::new();

    if let Some(young) = material.young_modulus {
        parts.push(format!("E={young:.3e}"));
    }
    if let Some(poisson) = material.poisson_ratio {
        parts.push(format!("nu={poisson:.3}"));
    }
    if let Some(density) = material.density {
        parts.push(format!("rho={density:.3e}"));
    }

    if parts.is_empty() {
        format!("[Mat] {}", material.name)
    } else {
        format!("[Mat] {}  {}", material.name, parts.join(" "))
    }
}

/// Formats one [`fem_core::Section`] as
/// `"[Sec] name  Shell t=2.0  mat=STEEL  (EGRP)"`, varying the
/// geometry field by [`fem_core::SectionKind`].
fn format_section_line(section: &fem_core::Section) -> String {
    let kind_label = match section.kind {
        fem_core::SectionKind::Solid => "Solid".to_string(),
        fem_core::SectionKind::Shell { thickness } => format!("Shell t={thickness:.3}"),
        fem_core::SectionKind::Beam { area } => format!("Beam A={area:.3}"),
    };

    let scope = section
        .element_set_name
        .as_deref()
        .map(|name| format!("  ({name})"))
        .unwrap_or_default();

    format!(
        "[Sec] {}  {kind_label}  mat={}{scope}",
        section.name, section.material_name,
    )
}
pub(crate) fn section_type_button_system(
    mut selected: ResMut<SelectedSectionType>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &SectionTypeButton,
        ),
        With<SectionTypeButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            *selected = btn.0;
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

/// Selects an EGRP for the section definition panel.
pub(crate) fn egrp_select_button_system(
    mut selected: ResMut<SelectedEgrp>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &EgrpSelectButton,
        ),
        With<EgrpSelectButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            selected.0 = btn.0.clone();
        }

        let active = selected.0 == btn.0;
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

/// Selects a material for the section definition panel.
pub(crate) fn material_select_button_system(
    mut selected: ResMut<SelectedMaterialForSection>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &MaterialSelectButton,
        ),
        With<MaterialSelectButton>,
    >,
) {
    for (interaction, mut bg, mut border, btn) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            selected.0 = Some(btn.0.clone());
        }

        let active = selected.0.as_deref() == Some(&btn.0);
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

/// Applies the configured section to [`fem_core::AnalysisSetup`].
pub(crate) fn add_section_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    section_type: Res<SelectedSectionType>,
    egrp: Res<SelectedEgrp>,
    material_sel: Res<SelectedMaterialForSection>,
    slider_query: Query<&SliderState, With<SliderTrack>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<AddSectionButton>,
    >,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let Some(mat_name) = &material_sel.0 else {
                continue;
            };

            let thickness = slider_query
                .iter()
                .find(|s| s.id == SliderId::SectionThickness)
                .map(|s| s.value)
                .unwrap_or(2.0);

            let kind = match *section_type {
                SelectedSectionType::Solid => fem_core::SectionKind::Solid,
                SelectedSectionType::Shell => fem_core::SectionKind::Shell { thickness },
                SelectedSectionType::Beam => fem_core::SectionKind::Beam { area: thickness },
            };

            setup.add_section(0, mat_name.clone(), egrp.0.clone(), kind);
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

/// Rebuilds the dynamic EGRP and material selector rows in the section
/// definition panel whenever the model or setup changes.
pub(crate) fn rebuild_section_def_panel(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    setup: Res<fem_core::AnalysisSetup>,
    version: Res<FemModelVersion>,
    mut last_ver: Local<Option<u64>>,
    egrp_row_q: Query<Entity, With<SectionDefEgrpRow>>,
    mat_row_q: Query<Entity, With<SectionDefMatRow>>,
    children_q: Query<&Children>,
) {
    let ver_changed = *last_ver != Some(version.value);
    *last_ver = Some(version.value);

    if !ver_changed && !setup.is_changed() {
        return;
    }

    // ── EGRP buttons ──
    if let Ok(egrp_row) = egrp_row_q.single() {
        if let Ok(children) = children_q.get(egrp_row) {
            for &c in children {
                commands.entity(c).despawn();
            }
        }

        commands.entity(egrp_row).with_children(|row| {
            // "ALL" option
            row.spawn((
                Button,
                Node {
                    padding: UiRect::axes(px(8.0), px(3.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(4.0)),
                    ..default()
                },
                BackgroundColor(BUTTON_NORMAL),
                BorderColor::all(PANEL_BORDER),
                EgrpSelectButton(None),
                Name::new("Egrp_ALL"),
            ))
            .with_child((
                Text::new("ALL"),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));

            if let Some(model) = model.as_deref() {
                for mesh in &model.meshes {
                    for eset in &mesh.element_sets {
                        let name = eset.name.clone();
                        row.spawn((
                            Button,
                            Node {
                                padding: UiRect::axes(px(8.0), px(3.0)),
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            EgrpSelectButton(Some(name.clone())),
                            Name::new(format!("Egrp_{name}")),
                        ))
                        .with_child((
                            Text::new(name),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    }
                }
            }
        });
    }

    // ── Material buttons ──
    if let Ok(mat_row) = mat_row_q.single() {
        if let Ok(children) = children_q.get(mat_row) {
            for &c in children {
                commands.entity(c).despawn();
            }
        }

        commands.entity(mat_row).with_children(|row| {
            for mat in &setup.materials {
                let name = mat.name.clone();
                row.spawn((
                    Button,
                    Node {
                        padding: UiRect::axes(px(8.0), px(3.0)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(4.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(PANEL_BORDER),
                    MaterialSelectButton(name.clone()),
                    Name::new(format!("MatSel_{name}")),
                ))
                .with_child((
                    Text::new(name),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));
            }
        });
    }
}
