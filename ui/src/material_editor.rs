//! Inline editing of the currently supported isotropic elastic constants.
//!
//! Text is a draft until Enter; changing the material, model, or page abandons
//! that draft. Values are in the model's unit system, never implicitly converted.

use crate::layout::{SidebarPage, UiInputCapture};
use crate::materials_ui::SelectedMaterialForSection;
use crate::measurement::editable_value;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, TextCursorStyle};
use bevy::ui_widgets::SelectAllOnFocus;
use fem_core::{AnalysisSetup, FemMaterial, FemModelVersion, UiKeyboardState};

const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const INPUT_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const INPUT_FOCUS: Color = Color::srgb(0.57, 0.86, 0.92);
const INPUT_ERROR: Color = Color::srgb(0.98, 0.36, 0.30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaterialField {
    Young,
    Poisson,
    Density,
}

impl MaterialField {
    const ALL: [Self; 3] = [Self::Young, Self::Poisson, Self::Density];

    fn label(self) -> &'static str {
        match self {
            Self::Young => "Young's modulus E",
            Self::Poisson => "Poisson ratio nu",
            Self::Density => "Density rho",
        }
    }

    fn value(self, material: &FemMaterial) -> Option<f32> {
        match self {
            Self::Young => material.young_modulus,
            Self::Poisson => material.poisson_ratio,
            Self::Density => material.density,
        }
    }

    fn set(self, material: &mut FemMaterial, value: Option<f32>) {
        match self {
            Self::Young => material.young_modulus = value,
            Self::Poisson => material.poisson_ratio = value,
            Self::Density => material.density = value,
        }
    }
}

#[derive(Component)]
pub(crate) struct MaterialValueInput(MaterialField);
#[derive(Component)]
pub(crate) struct MaterialFields;
#[derive(Component)]
pub(crate) struct MaterialEditorStatus;
#[derive(Component)]
pub(crate) struct MaterialEditorHeading;

/// Bitwise values let an imported NaN be displayed and repaired without
/// repeatedly cancelling focus (NaN != NaN under ordinary float equality).
#[derive(Debug, Clone, PartialEq, Eq)]
struct MaterialFingerprint {
    name: String,
    values: [Option<u32>; 3],
}

impl From<&FemMaterial> for MaterialFingerprint {
    fn from(material: &FemMaterial) -> Self {
        Self {
            name: material.name.clone(),
            values: MaterialField::ALL.map(|field| field.value(material).map(f32::to_bits)),
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct MaterialEditorState {
    context: Option<(u64, Option<MaterialFingerprint>)>,
    error: Option<(MaterialField, &'static str)>,
}

pub(crate) fn spawn_material_exact_editor(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(5.0),
                ..default()
            },
            Name::new("MaterialExactEditor"),
        ))
        .with_children(|editor| {
            editor.spawn((
                Text::new("Select or add a material"),
                TextFont { font_size: FontSize::Px(10.0), ..default() },
                TextColor(TEXT_MAIN),
                MaterialEditorHeading,
            ));
            editor
                .spawn((
                    Node {
                        display: Display::None,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(5.0),
                        ..default()
                    },
                    MaterialFields,
                ))
                .with_children(|fields| {
                    for field in MaterialField::ALL {
                        material_value_row(fields, field);
                    }
                    fields.spawn((
                        Text::new("Isotropic elastic | model units\nE: force/length^2; nu: dimensionless; rho: mass/length^3"),
                        TextFont { font_size: FontSize::Px(9.0), ..default() },
                        TextColor(TEXT_MUTED),
                    ));
                });
            editor.spawn((
                Text::new(""),
                TextFont { font_size: FontSize::Px(9.5), ..default() },
                TextColor(TEXT_MUTED),
                MaterialEditorStatus,
            ));
        });
}

fn material_value_row(parent: &mut ChildSpawnerCommands, field: MaterialField) {
    parent
        .spawn(Node {
            width: percent(100.0),
            height: px(27.0),
            align_items: AlignItems::Center,
            column_gap: px(5.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(field.label()),
                Node {
                    width: px(110.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));
            row.spawn((
                Node {
                    flex_grow: 1.0,
                    min_width: px(65.0),
                    height: percent(100.0),
                    padding: UiRect::axes(px(5.0), px(2.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(3.0)),
                    ..default()
                },
                EditableText {
                    visible_width: Some(14.0),
                    max_characters: Some(48),
                    allow_newlines: false,
                    ..EditableText::new("")
                },
                EditableTextFilter::new(|c| {
                    c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E')
                }),
                TextLayout {
                    justify: Justify::End,
                    ..TextLayout::no_wrap()
                },
                TextFont {
                    font_size: FontSize::Px(10.5),
                    ..default()
                },
                TextColor(TEXT_MAIN),
                TextCursorStyle {
                    color: INPUT_FOCUS,
                    ..default()
                },
                SelectAllOnFocus,
                BackgroundColor(Color::srgba(0.075, 0.09, 0.10, 0.98)),
                BorderColor::all(INPUT_BORDER),
                UiInputCapture,
                MaterialValueInput(field),
                Name::new(format!("MaterialInput_{field:?}")),
            ));
        });
}

fn parse_material_value(field: MaterialField, text: &str) -> Result<Option<f32>, &'static str> {
    if text.trim().is_empty() && field == MaterialField::Density {
        return Ok(None);
    }
    let parsed = text
        .trim()
        .parse::<f64>()
        .map_err(|_| "enter a finite number")?;
    let value = parsed as f32;
    if !parsed.is_finite() || !value.is_finite() {
        return Err("enter a finite number");
    }
    let nonzero_mantissa = text
        .trim()
        .split(['e', 'E'])
        .next()
        .unwrap_or_default()
        .bytes()
        .any(|digit| matches!(digit, b'1'..=b'9'));
    if nonzero_mantissa && value == 0.0 {
        return Err("value is too small for the supported precision");
    }
    match field {
        MaterialField::Young | MaterialField::Density if value <= 0.0 => {
            Err("must be greater than zero")
        }
        MaterialField::Poisson if value <= -1.0 || value >= 0.5 => {
            Err("must be greater than -1 and less than 0.5")
        }
        _ => Ok(Some(value)),
    }
}

fn value_text(value: Option<f32>) -> String {
    // Shortest round-tripping representation of the stored f32. Formatting to
    // a fixed six decimals would silently alter very small density/nu values.
    value.map(|value| value.to_string()).unwrap_or_default()
}

pub(crate) fn material_numeric_input_system(
    page: Res<SidebarPage>,
    version: Res<FemModelVersion>,
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
    mut keyboard: ResMut<UiKeyboardState>,
    mut selected: ResMut<SelectedMaterialForSection>,
    mut setup: ResMut<AnalysisSetup>,
    mut state: ResMut<MaterialEditorState>,
    mut inputs: Query<(
        Entity,
        &MaterialValueInput,
        &mut EditableText,
        &mut BorderColor,
    )>,
    mut field_panels: Query<&mut Node, With<MaterialFields>>,
    mut headings: Query<&mut Text, With<MaterialEditorHeading>>,
    mut statuses: Query<
        (&mut Text, &mut TextColor),
        (With<MaterialEditorStatus>, Without<MaterialEditorHeading>),
    >,
) {
    if !selected
        .0
        .as_ref()
        .is_some_and(|name| setup.material_by_name(name).is_some())
    {
        if selected.0.is_some() {
            selected.0 = None;
        }
    }
    let index = selected.0.as_ref().and_then(|name| {
        let mut matches = setup
            .materials
            .iter()
            .enumerate()
            .filter(|(_, material)| material.name == *name);
        let (index, _) = matches.next()?;
        matches.next().is_none().then_some(index)
    });
    let fingerprint = index.map(|index| MaterialFingerprint::from(&setup.materials[index]));
    let context = (version.value, fingerprint);
    let context_changed = state.context.as_ref() != Some(&context);
    let active = *page == SidebarPage::Materials && index.is_some();
    if context_changed || !active {
        if focus.get().is_some_and(|entity| inputs.contains(entity)) {
            focus.clear();
            keyboard.text_editing = true;
        }
        state.error = None;
        state.context = Some(context);
    }

    let focused = focus.get();
    state.error = None;
    for (entity, field, mut input, mut border) in &mut inputs {
        let is_focused = active && focused == Some(entity);
        if is_focused && !context_changed {
            if keys.just_pressed(KeyCode::Escape) {
                focus.clear();
                keyboard.text_editing = true;
                state.error = None;
            } else {
                match parse_material_value(field.0, &editable_value(&input)) {
                    Ok(value) => {
                        state.error = None;
                        if keys.just_pressed(KeyCode::Enter) && !input.is_composing() {
                            let index = index.unwrap();
                            if field.0.value(&setup.materials[index]) != value {
                                field.0.set(&mut setup.materials[index], value);
                            }
                            state.context = Some((
                                version.value,
                                Some(MaterialFingerprint::from(&setup.materials[index])),
                            ));
                            focus.clear();
                            keyboard.text_editing = true;
                        }
                    }
                    Err(error) => state.error = Some((field.0, error)),
                }
            }
        }
        if focus.get() != Some(entity) || !active {
            let desired =
                value_text(index.and_then(|index| field.0.value(&setup.materials[index])));
            if editable_value(&input) != desired {
                input.editor_mut().set_text(&desired);
                input.queue_edit(bevy::text::TextEdit::TextEnd(false));
            }
        }
        let error = state
            .error
            .is_some_and(|(error_field, _)| field.0 == error_field);
        *border = BorderColor::all(if error {
            INPUT_ERROR
        } else if focus.get() == Some(entity) {
            INPUT_FOCUS
        } else {
            INPUT_BORDER
        });
    }
    for mut panel in &mut field_panels {
        let display = if index.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if panel.display != display {
            panel.display = display;
        }
    }
    let heading = index
        .map(|index| {
            let name = &setup.materials[index].name;
            let uses = setup
                .sections
                .iter()
                .filter(|section| section.material_name == *name)
                .count();
            format!("{name} | used by {uses} section(s)")
        })
        .unwrap_or_else(|| "Select or add a material".to_string());
    for mut text in &mut headings {
        text.set_if_neq(Text::new(&heading));
    }
    let status = if let Some((field, error)) = state.error {
        format!("{}: {error}; Esc restores", field.label())
    } else if index.is_none() && selected.0.is_some() {
        "Duplicate material names; resolve before editing".to_string()
    } else if index.is_none() {
        "Choose a project material to edit, or select a library material and confirm the assignment".to_string()
    } else {
        "Enter applies; Esc restores. Blank rho = unspecified. Editing affects all sections using this material.".to_string()
    };
    for (mut text, mut color) in &mut statuses {
        text.set_if_neq(Text::new(&status));
        color.set_if_neq(TextColor(if state.error.is_some() {
            INPUT_ERROR
        } else {
            TEXT_MUTED
        }));
    }
}

#[cfg(test)]
#[path = "material_editor_tests.rs"]
mod tests;
