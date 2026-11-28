//! Draft editing for supported von Mises hardening laws.

use crate::{
    ButtonInteraction, InteractionButton,
    layout::{SidebarPage, UiInputCapture},
    material_editor::unique_material_index,
    materials_ui::SelectedMaterialForSection,
    measurement::editable_value,
};
use bevy::{
    input_focus::{InputFocus, tab_navigation::TabIndex},
    picking::Pickable,
    prelude::*,
    text::{EditableText, TextCursorStyle},
    ui_widgets::{Activate, Button as WidgetButton, SelectAllOnFocus, TextInput},
};
use fem_core::{
    AnalysisSetup, FemMaterial, FemModelVersion, MisesPlasticity, PlasticHardeningPoint,
    UiKeyboardState,
};

const FOREGROUND: Color = Color::srgb(0.88, 0.92, 0.94);
const BORDER: Color = Color::srgb(0.34, 0.40, 0.44);
const FOCUS: Color = Color::srgb(0.57, 0.86, 0.92);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Law {
    Elastic,
    Bilinear,
    Multilinear,
}

#[derive(Component, Clone, Copy)]
pub(crate) enum Action {
    Open,
    Select(Law),
    Apply,
    Cancel,
}

#[derive(Component)]
pub(crate) enum Panel {
    Root,
    Open,
    Draft,
    Values(Law),
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    InitialYield,
    HardeningModulus,
    Curve,
}

#[derive(Component)]
pub(crate) struct Status;

struct Draft {
    material_name: String,
    model_version: u64,
    original: Option<MisesPlasticity>,
    law: Law,
    error: String,
}

#[derive(Resource, Default)]
pub(crate) struct PlasticityEditor {
    draft: Option<Draft>,
    pending: Option<Action>,
}

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((vertical_panel(), Panel::Root, Visibility::Hidden))
        .with_children(|root| {
            button(root, "Edit plasticity...", Action::Open, "PlasticityOpen");
            root.spawn((vertical_panel(), Panel::Draft, Visibility::Hidden))
                .with_children(|editor| {
                    editor.spawn(label("Material law"));
                    editor.spawn(button_row()).with_children(|row| {
                        for (law, label, name) in [
                            (Law::Elastic, "Elastic", "PlasticityElastic"),
                            (Law::Bilinear, "Bilinear", "PlasticityBilinear"),
                            (Law::Multilinear, "Multilinear", "PlasticityMultilinear"),
                        ] {
                            button(row, label, Action::Select(law), name);
                        }
                    });
                    editor
                        .spawn((
                            vertical_panel(),
                            Panel::Values(Law::Bilinear),
                            Visibility::Hidden,
                        ))
                        .with_children(|fields| {
                            value_input(
                                fields,
                                Field::InitialYield,
                                "Initial yield stress",
                                "PlasticityYield",
                            );
                            value_input(
                                fields,
                                Field::HardeningModulus,
                                "Hardening modulus",
                                "PlasticityModulus",
                            );
                        });
                    editor
                        .spawn((
                            vertical_panel(),
                            Panel::Values(Law::Multilinear),
                            Visibility::Hidden,
                        ))
                        .with_children(|fields| {
                            value_input(
                                fields,
                                Field::Curve,
                                "Yield stress, plastic strain (one pair per line)",
                                "PlasticityCurve",
                            );
                        });
                    editor.spawn((label(""), Status));
                    editor.spawn(button_row()).with_children(|row| {
                        button(row, "Apply plasticity", Action::Apply, "PlasticityApply");
                        button(row, "Cancel", Action::Cancel, "PlasticityCancel");
                    });
                });
        });
}

fn vertical_panel() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(4.0),
        display: Display::None,
        ..default()
    }
}

fn button_row() -> Node {
    Node {
        column_gap: px(4.0),
        ..default()
    }
}

fn label(value: &str) -> impl Bundle {
    (
        Text::new(value),
        Pickable::IGNORE,
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(FOREGROUND),
    )
}

fn button(parent: &mut ChildSpawnerCommands, text: &str, action: Action, name: &'static str) {
    let display = if matches!(action, Action::Open) {
        Display::None
    } else {
        Display::Flex
    };
    let entity = parent
        .spawn((
            action,
            InteractionButton,
            WidgetButton,
            TabIndex(0),
            Node {
                display,
                flex_grow: 1.0,
                min_height: px(25.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(4.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BorderColor::all(BORDER),
            BackgroundColor(Color::srgb(0.10, 0.12, 0.14)),
            Name::new(name),
        ))
        .observe(queue_action)
        .with_child(label(text))
        .id();
    if matches!(action, Action::Open) {
        parent
            .commands()
            .entity(entity)
            .insert((Panel::Open, Visibility::Hidden));
    }
}

fn value_input(parent: &mut ChildSpawnerCommands, field: Field, prompt: &str, name: &'static str) {
    let multiline = field == Field::Curve;
    parent.spawn(label(prompt));
    let mut input = parent.spawn((
        field,
        TextInput,
        UiInputCapture,
        TabIndex(0),
        EditableText {
            allow_newlines: multiline,
            visible_lines: Some(if multiline { 7.0 } else { 1.0 }),
            ..EditableText::new("")
        },
        TextLayout::no_wrap(),
        TextFont {
            font_size: FontSize::Px(10.5),
            ..default()
        },
        TextColor(FOREGROUND),
        TextCursorStyle {
            color: FOCUS,
            ..default()
        },
        BorderColor::all(BORDER),
        BackgroundColor(Color::srgb(0.075, 0.09, 0.10)),
        Node {
            width: percent(100.0),
            min_height: px(24.0),
            padding: UiRect::axes(px(5.0), px(3.0)),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(3.0)),
            ..default()
        },
        Name::new(name),
    ));
    if !multiline {
        input.insert(SelectAllOnFocus);
    }
}

fn queue_action(
    event: On<Activate>,
    buttons: Query<&Action>,
    mut editor: ResMut<PlasticityEditor>,
) {
    if let Ok(&action) = buttons.get(event.entity) {
        editor.pending = Some(action);
    }
}

pub(crate) fn update(
    page: Res<SidebarPage>,
    version: Res<FemModelVersion>,
    selected: Res<SelectedMaterialForSection>,
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    mut keyboard: ResMut<UiKeyboardState>,
    mut setup: ResMut<AnalysisSetup>,
    mut editor: ResMut<PlasticityEditor>,
    mut inputs: Query<(&Field, &mut EditableText)>,
    mut panels: Query<(&Panel, &mut Node, &mut Visibility)>,
    mut buttons: Query<(&Action, &ButtonInteraction, &mut BackgroundColor)>,
    mut status: Query<(&mut Text, &mut TextColor), With<Status>>,
) {
    let index = (*page == SidebarPage::Materials)
        .then(|| unique_material_index(&setup, selected.0.as_deref()))
        .flatten()
        .filter(|&index| !setup.materials[index].is_fluid());
    let mut pending = editor.pending.take();
    if editor.draft.as_ref().is_some_and(|draft| {
        draft.model_version != version.value
            || index.is_none_or(|index| {
                let current = &setup.materials[index];
                current.name != draft.material_name || current.plasticity != draft.original
            })
    }) {
        editor.draft = None;
        clear_editor_focus(&mut focus, &inputs, &buttons);
        pending = None;
    }
    if editor.draft.is_some() && keys.just_pressed(KeyCode::Escape) {
        pending = Some(Action::Cancel);
        keyboard.text_editing = true;
    }
    match pending {
        Some(Action::Open) if editor.draft.is_none() => {
            if let Some(material) = index.map(|index| &setup.materials[index]) {
                editor.draft = Some(open_draft(material, version.value, &mut inputs));
                clear_editor_focus(&mut focus, &inputs, &buttons);
            }
        }
        Some(Action::Select(law)) => {
            if let Some(draft) = editor.draft.as_mut() {
                draft.law = law;
                draft.error.clear();
                if focus.get().is_some_and(|entity| inputs.contains(entity)) {
                    focus.clear();
                }
            }
        }
        Some(Action::Apply) => {
            if let (Some(index), Some(draft)) = (index, editor.draft.as_mut()) {
                match parse_draft(draft.law, &inputs) {
                    Ok(plasticity) => {
                        if plasticity != draft.original {
                            setup.materials[index].plasticity = plasticity;
                        }
                        editor.draft = None;
                        clear_editor_focus(&mut focus, &inputs, &buttons);
                    }
                    Err(error) => draft.error = error,
                }
            }
        }
        Some(Action::Cancel) => {
            editor.draft = None;
            clear_editor_focus(&mut focus, &inputs, &buttons);
        }
        _ => {}
    }

    let law = editor.draft.as_ref().map(|draft| draft.law);
    for (panel, mut node, mut visibility) in &mut panels {
        let visible = match panel {
            Panel::Root => index.is_some(),
            Panel::Open => law.is_none(),
            Panel::Draft => law.is_some(),
            Panel::Values(kind) => law == Some(*kind),
        };
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        visibility.set_if_neq(if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (action, interaction, mut color) in &mut buttons {
        let selected = matches!(action, Action::Select(kind) if law == Some(*kind));
        let value = match (interaction, selected) {
            (ButtonInteraction::Pressed, _) => Color::srgb(0.22, 0.55, 0.66),
            (_, true) => Color::srgb(0.18, 0.45, 0.55),
            (ButtonInteraction::Hovered, false) => Color::srgb(0.18, 0.22, 0.24),
            _ => Color::srgb(0.10, 0.12, 0.14),
        };
        color.set_if_neq(BackgroundColor(value));
    }
    let error = editor
        .draft
        .as_ref()
        .map_or("", |draft| draft.error.as_str());
    for (mut text, mut color) in &mut status {
        text.set_if_neq(Text::new(if error.is_empty() {
            "Stress and hardening modulus use model stress units."
        } else {
            error
        }));
        color.set_if_neq(TextColor(if error.is_empty() {
            FOREGROUND
        } else {
            Color::srgb(0.98, 0.48, 0.36)
        }));
    }
}

fn clear_editor_focus(
    focus: &mut InputFocus,
    inputs: &Query<(&Field, &mut EditableText)>,
    buttons: &Query<(&Action, &ButtonInteraction, &mut BackgroundColor)>,
) {
    if focus
        .get()
        .is_some_and(|entity| inputs.contains(entity) || buttons.contains(entity))
    {
        focus.clear();
    }
}

fn open_draft(
    material: &FemMaterial,
    model_version: u64,
    inputs: &mut Query<(&Field, &mut EditableText)>,
) -> Draft {
    let (law, yield_stress, modulus, curve) = match &material.plasticity {
        None => (Law::Elastic, String::new(), String::new(), String::new()),
        Some(MisesPlasticity::Bilinear {
            initial_yield_stress,
            hardening_modulus,
        }) => (
            Law::Bilinear,
            format!("{initial_yield_stress:e}"),
            format!("{hardening_modulus:e}"),
            String::new(),
        ),
        Some(MisesPlasticity::Multilinear(points)) => (
            Law::Multilinear,
            String::new(),
            String::new(),
            points
                .iter()
                .map(|point| format!("{:e}, {:e}", point.yield_stress, point.plastic_strain))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    };
    for (field, mut input) in inputs {
        input.clear();
        input.editor_mut().set_text(match field {
            Field::InitialYield => &yield_stress,
            Field::HardeningModulus => &modulus,
            Field::Curve => &curve,
        });
    }
    Draft {
        material_name: material.name.clone(),
        model_version,
        original: material.plasticity.clone(),
        law,
        error: String::new(),
    }
}

fn parse_draft(
    law: Law,
    inputs: &Query<(&Field, &mut EditableText)>,
) -> Result<Option<MisesPlasticity>, String> {
    let value = |field| -> Result<String, &'static str> {
        let (_, input) = inputs
            .iter()
            .find(|(kind, _)| **kind == field)
            .ok_or("Plasticity input is unavailable")?;
        if input.is_composing() || input.pending_paste.is_some() {
            return Err("Finish entering the value before applying");
        }
        Ok(editable_value(input))
    };
    let plasticity = match law {
        Law::Elastic => return Ok(None),
        Law::Bilinear => MisesPlasticity::Bilinear {
            initial_yield_stress: parse_number(&value(Field::InitialYield)?)
                .ok_or("Enter a finite initial yield stress")?,
            hardening_modulus: parse_number(&value(Field::HardeningModulus)?)
                .ok_or("Enter a finite hardening modulus")?,
        },
        Law::Multilinear => MisesPlasticity::Multilinear(parse_curve(&value(Field::Curve)?)?),
    };
    plasticity.validate().map_err(str::to_owned)?;
    Ok(Some(plasticity))
}

fn parse_curve(text: &str) -> Result<Vec<PlasticHardeningPoint>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let columns = line.split_once(',').or_else(|| {
                let mut columns = line.split_whitespace();
                Some((columns.next()?, columns.next()?)).filter(|_| columns.next().is_none())
            });
            columns
                .and_then(|(stress, strain)| {
                    Some(PlasticHardeningPoint {
                        yield_stress: parse_number(stress)?,
                        plastic_strain: parse_number(strain)?,
                    })
                })
                .ok_or_else(|| {
                    format!(
                        "Row {} needs a finite yield stress and plastic strain",
                        index + 1
                    )
                })
        })
        .collect()
}

fn parse_number(text: &str) -> Option<f64> {
    let text = text.trim().replace(['d', 'D', 'E'], "e");
    let value = text.parse::<f64>().ok()?;
    let underflow = value == 0.0
        && text
            .split('e')
            .next()
            .unwrap_or_default()
            .bytes()
            .any(|digit| matches!(digit, b'1'..=b'9'));
    (value.is_finite() && !underflow).then_some(value)
}
