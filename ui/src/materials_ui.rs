use crate::layout::{DeleteSetupEntry, ScrollableList, SidebarPage, SidebarPageContent};
use crate::material_editor::spawn_material_exact_editor;
use crate::material_library::{
    MaterialLibraryState, resolved_material_name, spawn_material_library, use_material,
};
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

#[cfg(test)]
#[path = "material_assignment_tests.rs"]
mod tests;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SelectedSectionType {
    #[default]
    Solid,
    Shell,
    Beam,
}

#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct SelectedEgrp(pub Option<AssignmentTarget>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssignmentTarget {
    pub mesh_index: usize,
    pub group: Option<String>,
}

pub(crate) fn choose_assignment_target(
    selected: &mut SelectedEgrp,
    target: Option<AssignmentTarget>,
    material: &mut SelectedMaterialForSection,
    library: &mut MaterialLibraryState,
) {
    if selected.0 != target {
        selected.0 = target;
        material.0 = None;
        library.selected = None;
    }
}

fn valid_target<'a>(
    target: &'a SelectedEgrp,
    model: Option<&FemModel>,
) -> Option<&'a AssignmentTarget> {
    target.0.as_ref().filter(|target| {
        model
            .and_then(|m| m.meshes.get(target.mesh_index))
            .is_some_and(|mesh| {
                target
                    .group
                    .as_ref()
                    .is_none_or(|name| mesh.element_sets.iter().any(|g| &g.name == name))
            })
    })
}

fn needs_new_section(setup: &fem_core::AnalysisSetup, target: &AssignmentTarget) -> bool {
    !setup
        .sections
        .iter()
        .any(|s| s.mesh_index == target.mesh_index && s.element_set_name == target.group)
}

fn target_label(target: &AssignmentTarget, model: &FemModel) -> String {
    let name = model
        .parts
        .iter()
        .find(|p| p.mesh_index == target.mesh_index)
        .map(|p| p.name.as_str())
        .unwrap_or("Mesh");
    format!(
        "[{}] {} / {}",
        target.mesh_index + 1,
        name,
        target.group.as_deref().unwrap_or("whole part")
    )
}

pub(crate) fn update_material_workflow(
    target: Res<SelectedEgrp>,
    model: Option<Res<FemModel>>,
    setup: Res<fem_core::AnalysisSetup>,
    kind: Res<SelectedSectionType>,
    mut steps: Query<
        &mut Node,
        (
            With<MaterialAfterTarget>,
            Without<NewSectionControls>,
            Without<SectionSizeControls>,
        ),
    >,
    mut defaults: Query<
        &mut Node,
        (
            With<NewSectionControls>,
            Without<MaterialAfterTarget>,
            Without<SectionSizeControls>,
        ),
    >,
    mut sizes: Query<
        &mut Node,
        (
            With<SectionSizeControls>,
            Without<MaterialAfterTarget>,
            Without<NewSectionControls>,
        ),
    >,
    mut labels: Query<&mut Text, With<MaterialTargetStatus>>,
) {
    let target = valid_target(&target, model.as_deref());
    for mut node in &mut steps {
        node.display = if target.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut defaults {
        node.display = if target.is_some_and(|t| needs_new_section(&setup, t)) {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut sizes {
        node.display = if *kind == SelectedSectionType::Solid {
            Display::None
        } else {
            Display::Flex
        };
    }
    let text = target
        .map(|t| format!("Selected: {}", target_label(t, model.as_deref().unwrap())))
        .unwrap_or_else(|| "Click a part in the viewport, or choose a part/group below".into());
    for mut label in &mut labels {
        label.set_if_neq(Text::new(&text));
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct SelectedMaterialForSection(pub Option<String>);

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SectionTypeButton(pub SelectedSectionType);

#[derive(Component, Debug, Clone)]
pub(crate) struct EgrpSelectButton(pub AssignmentTarget);

#[derive(Component, Debug, Clone)]
pub(crate) struct MaterialSelectButton(pub String);

#[derive(Component)]
pub(crate) struct AddSectionButton;
#[derive(Component)]
pub(crate) struct AddSectionLabel;

#[derive(Component)]
pub(crate) struct SectionDefEgrpRow;

#[derive(Component)]
pub(crate) struct MaterialSelectorRow;

#[derive(Component)]
pub(crate) struct MaterialsSectionsListContainer;

#[derive(Component)]
pub(crate) struct MaterialAfterTarget;
#[derive(Component)]
pub(crate) struct MaterialTargetStatus;
#[derive(Component)]
pub(crate) struct NewSectionControls;
#[derive(Component)]
pub(crate) struct SectionSizeControls;

fn material_heading(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(TEXT_MAIN),
    ));
}
fn material_step_node() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(5.0),
        margin: UiRect::top(px(6.0)),
        ..default()
    }
}

pub(crate) fn spawn_materials_ui(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(5.0),
                padding: UiRect::all(px(6.0)),
                ..default()
            },
            SidebarPageContent::page(SidebarPage::Materials),
            Name::new("MaterialsEditorPanel"),
        ))
        .with_children(|panel| {
            material_heading(panel, "1 | SELECT OBJECT");
            panel.spawn((
                Text::new("Click a part in the viewport, or choose a part/group below"),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
                MaterialTargetStatus,
            ));
            panel.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4.0),
                    max_height: px(125.0),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ScrollPosition::default(),
                ScrollableList,
                SectionDefEgrpRow,
                Name::new("SectionDefEgrpRow"),
            ));
            panel
                .spawn((
                    material_step_node(),
                    MaterialAfterTarget,
                    Name::new("MaterialChoiceStep"),
                ))
                .with_children(|choice| {
                    material_heading(choice, "2 | SELECT MATERIAL");
                    material_heading(choice, "Materials already in this model");
                    choice.spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(4.0),
                            flex_wrap: FlexWrap::Wrap,
                            row_gap: px(4.0),
                            ..default()
                        },
                        MaterialSelectorRow,
                        Name::new("MaterialSelectorRow"),
                    ));
                    spawn_material_library(choice);
                    spawn_material_exact_editor(choice);
                });
            panel
                .spawn((
                    material_step_node(),
                    MaterialAfterTarget,
                    Name::new("SectionDefPanel"),
                ))
                .with_children(|section| {
                    material_heading(section, "3 | CONFIRM ASSIGNMENT");
                    section
                        .spawn((material_step_node(), NewSectionControls))
                        .with_children(|defaults| {
                            material_heading(
                                defaults,
                                "New section type (existing thickness/area is kept)",
                            );
                            defaults
                                .spawn(Node {
                                    flex_direction: FlexDirection::Row,
                                    column_gap: px(4.0),
                                    ..default()
                                })
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
                            defaults
                                .spawn((material_step_node(), SectionSizeControls))
                                .with_children(|size| {
                                    spawn_slider(
                                        size,
                                        SliderConfig {
                                            width: 268.0,
                                            min: 0.0,
                                            max: 50.0,
                                            value: 2.0,
                                            label: "Thickness / Area",
                                            id: SliderId::SectionThickness,
                                        },
                                    );
                                });
                        });
                    section
                        .spawn((
                            Button,
                            Node {
                                width: percent(100.0),
                                min_height: px(30.0),
                                padding: UiRect::all(px(4.0)),
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
                            Text::new("Choose a material"),
                            TextFont {
                                font_size: FontSize::Px(10.5),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                            AddSectionLabel,
                        ));
                    material_heading(
                        section,
                        "Only confirmation changes the assignment. Ctrl+Z undoes it.",
                    );
                });
        });
    parent.spawn((
        Text::new("Materials & sections:"),
        TextFont {
            font_size: FontSize::Px(9.5),
            ..default()
        },
        TextColor(TEXT_MAIN),
        SidebarPageContent::page(SidebarPage::Materials),
    ));
    parent.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3.0),
            max_height: px(90.0),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        ScrollPosition::default(),
        ScrollableList,
        MaterialsSectionsListContainer,
        SidebarPageContent::page(SidebarPage::Materials),
        Name::new("MaterialsSectionsListContainer"),
    ));
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
    mut material: ResMut<SelectedMaterialForSection>,
    mut library: ResMut<MaterialLibraryState>,
    page: Res<SidebarPage>,
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
        if *page == SidebarPage::Materials
            && *interaction == Interaction::Pressed
            && interaction.is_changed()
        {
            choose_assignment_target(
                &mut selected,
                Some(btn.0.clone()),
                &mut material,
                &mut library,
            );
        }

        let active = selected.0.as_ref() == Some(&btn.0);
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
    target: Res<SelectedEgrp>,
    mut library: ResMut<MaterialLibraryState>,
    page: Res<SidebarPage>,
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
        if *page == SidebarPage::Materials
            && target.0.is_some()
            && *interaction == Interaction::Pressed
            && interaction.is_changed()
        {
            selected.0 = Some(btn.0.clone());
            library.selected = None;
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

/// Assignment changes material identity, preserving existing section geometry.
fn assign_material(
    setup: &mut fem_core::AnalysisSetup,
    target: &AssignmentTarget,
    name: &str,
    kind: fem_core::SectionKind,
) -> bool {
    let matching: Vec<_> = setup
        .sections
        .iter()
        .enumerate()
        .filter(|(_, section)| {
            section.mesh_index == target.mesh_index
                && (target.group.is_none() || section.element_set_name == target.group)
        })
        .map(|(index, _)| index)
        .collect();
    if matching.is_empty() {
        setup.add_section(
            target.mesh_index,
            name.to_string(),
            target.group.clone(),
            kind,
        );
        return true;
    }
    let mut changed = false;
    for index in matching {
        if setup.sections[index].material_name != name {
            setup.sections[index].material_name = name.to_string();
            changed = true;
        }
    }
    // A whole-part assignment must also cover elements outside named groups.
    if target.group.is_none()
        && !setup.sections.iter().any(|section| {
            section.mesh_index == target.mesh_index && section.element_set_name.is_none()
        })
    {
        setup.add_section(target.mesh_index, name, None, kind);
        changed = true;
    }
    changed
}

pub(crate) fn add_section_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    model: Option<Res<FemModel>>,
    page: Res<SidebarPage>,
    section_type: Res<SelectedSectionType>,
    egrp: Res<SelectedEgrp>,
    mut material_sel: ResMut<SelectedMaterialForSection>,
    mut library: ResMut<MaterialLibraryState>,
    slider_query: Query<&SliderState, With<SliderTrack>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<AddSectionButton>,
    >,
    mut labels: Query<&mut Text, With<AddSectionLabel>>,
) {
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
    let target = valid_target(&egrp, model.as_deref());
    let draft = library.draft();
    let name = if library.selected.is_some() {
        draft.as_ref().map(|m| resolved_material_name(&setup, m))
    } else {
        material_sel
            .0
            .as_ref()
            .filter(|name| setup.materials.iter().filter(|m| &m.name == *name).count() == 1)
            .cloned()
    };
    let valid_size = target.is_none_or(|t| !needs_new_section(&setup, t))
        || *section_type == SelectedSectionType::Solid
        || (thickness.is_finite() && thickness > 0.0);
    let enabled =
        *page == SidebarPage::Materials && target.is_some() && name.is_some() && valid_size;
    for (interaction, mut bg, mut border) in &mut buttons {
        if enabled && *interaction == Interaction::Pressed && interaction.is_changed() {
            let before = setup.materials.len();
            let final_name = if let Some(draft) = draft.clone() {
                use_material(setup.bypass_change_detection(), draft)
            } else {
                name.clone().unwrap()
            };
            let assigned = assign_material(
                setup.bypass_change_detection(),
                target.unwrap(),
                &final_name,
                kind,
            );
            if assigned || setup.materials.len() != before {
                setup.set_changed();
            }
            material_sel.0 = Some(final_name);
            library.selected = None;
        }
        bg.set_if_neq(BackgroundColor(if !enabled {
            Color::srgb(0.06, 0.07, 0.08)
        } else {
            match *interaction {
                Interaction::Pressed => BUTTON_PRESSED,
                Interaction::Hovered => BUTTON_HOVERED,
                _ => BUTTON_NORMAL,
            }
        }));
        border.set_if_neq(BorderColor::all(PANEL_BORDER));
    }
    let text = if target.is_none() {
        "Select an object first".into()
    } else if !valid_size {
        "Thickness / area must be positive".into()
    } else if let Some(name) = &name {
        format!(
            "Confirm: {name} -> {}",
            target_label(target.unwrap(), model.as_deref().unwrap())
        )
    } else if library.selected.is_some() {
        "Choose model units before confirming".into()
    } else {
        "Choose a material above".into()
    };
    for mut label in &mut labels {
        label.set_if_neq(Text::new(&text));
    }
}

fn assignment_button(row: &mut ChildSpawnerCommands, target: AssignmentTarget, label: String) {
    let key = match &target.group {
        None => format!("Assignment_{}_WHOLE", target.mesh_index),
        Some(group) => format!("Assignment_{}_GROUP_{group}", target.mesh_index),
    };
    row.spawn((
        Button,
        Node {
            padding: UiRect::axes(px(6.0), px(3.0)),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(4.0)),
            ..default()
        },
        BackgroundColor(BUTTON_NORMAL),
        BorderColor::all(PANEL_BORDER),
        EgrpSelectButton(target),
        Name::new(key),
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

/// Rebuilds the dynamic EGRP and material selector rows in the section
/// definition panel whenever the model or setup changes.
pub(crate) fn rebuild_section_def_panel(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    setup: Res<fem_core::AnalysisSetup>,
    version: Res<FemModelVersion>,
    mut last_ver: Local<Option<u64>>,
    mut target: ResMut<SelectedEgrp>,
    mut selected_material: ResMut<SelectedMaterialForSection>,
    mut library: ResMut<MaterialLibraryState>,
    egrp_row_q: Query<Entity, With<SectionDefEgrpRow>>,
    mat_row_q: Query<Entity, With<MaterialSelectorRow>>,
    children_q: Query<&Children>,
) {
    let ver_changed = *last_ver != Some(version.value);
    if ver_changed && last_ver.is_some() {
        choose_assignment_target(&mut target, None, &mut selected_material, &mut library);
    }
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
            if let Some(model) = model.as_deref() {
                for (mesh_index, mesh) in model.meshes.iter().enumerate() {
                    let part_name = model
                        .parts
                        .iter()
                        .find(|part| part.mesh_index == mesh_index)
                        .map(|part| part.name.as_str())
                        .unwrap_or("Mesh");
                    assignment_button(
                        row,
                        AssignmentTarget {
                            mesh_index,
                            group: None,
                        },
                        format!("[{}] {part_name} / whole part", mesh_index + 1),
                    );
                    for group in &mesh.element_sets {
                        assignment_button(
                            row,
                            AssignmentTarget {
                                mesh_index,
                                group: Some(group.name.clone()),
                            },
                            format!("[{}] {}", mesh_index + 1, group.name),
                        );
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
                .with_children(|button| {
                    button.spawn((
                        Node {
                            width: px(10.0),
                            height: px(10.0),
                            margin: UiRect::right(px(4.0)),
                            ..default()
                        },
                        BackgroundColor(visualization::material_identity_color(&name)),
                    ));
                    button.spawn((
                        Text::new(name),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(TEXT_MAIN),
                    ));
                });
            }
        });
    }
}

#[derive(Component)]
pub(crate) struct MaterialColorButton(visualization::MaterialColorMode);

pub(crate) fn spawn_material_color_controls(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(4.0),
            ..default()
        })
        .with_children(|row| {
            for (mode, label) in [
                (visualization::MaterialColorMode::Part, "Color: Part"),
                (
                    visualization::MaterialColorMode::Material,
                    "Color: Material",
                ),
            ] {
                row.spawn((
                    Button,
                    Node {
                        flex_grow: 1.0,
                        height: px(23.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(4.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(PANEL_BORDER),
                    MaterialColorButton(mode),
                    Name::new(label),
                ))
                .with_child((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));
            }
        });
}

pub(crate) fn material_color_button_system(
    mut mode: ResMut<visualization::MaterialColorMode>,
    mut buttons: Query<(Ref<Interaction>, &MaterialColorButton, &mut BackgroundColor)>,
) {
    for (interaction, button, _) in &buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            mode.set_if_neq(button.0);
        }
    }
    for (interaction, button, mut background) in &mut buttons {
        *background = BackgroundColor(if *mode == button.0 {
            BUTTON_ACTIVE
        } else if *interaction != Interaction::None {
            BUTTON_HOVERED
        } else {
            BUTTON_NORMAL
        });
    }
}
