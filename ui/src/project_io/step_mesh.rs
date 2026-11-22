//! STEP meshing controls and standalone FrontISTR mesh export.

use super::{BUTTON_HOVERED, BUTTON_NORMAL, BUTTON_PRESSED, PANEL_BORDER, TEXT_MAIN, TEXT_MUTED};
use crate::{
    ButtonInteraction, InteractionButton,
    assembly::AssemblyEditorState,
    step_import::{StepImport, spawn_step_import_ui},
};
use bevy::{
    feathers::controls::{
        FeathersNumberInput, HardLimit, NumberInputPrecision, NumberInputRange, NumberInputStep,
        NumberInputValue,
    },
    prelude::*,
    ui_widgets::ValueChange,
};
use fem_core::FemModel;
use gmsh::StepMeshOptions;
use std::path::Path;

const DEFAULT_STEP_CURVE_RESOLUTION: i32 = StepMeshOptions::DEFAULT_ELEMENTS_PER_TURN as i32;
const MAX_STEP_CURVE_RESOLUTION: i32 = 100;

#[derive(Resource, Default)]
pub(crate) struct StepMeshSettings {
    pub(crate) options: StepMeshOptions,
}

#[derive(Component)]
struct StepMeshSize;

#[derive(Component)]
struct StepCurveResolution;

#[derive(Component)]
pub(crate) struct SaveStepMeshButton;

#[derive(Component)]
pub(crate) struct SaveStepMeshControls;

#[derive(Component)]
pub(crate) struct SaveStepMeshStatus;

pub(super) fn is_step(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("step") || extension.eq_ignore_ascii_case("stp")
        })
}

fn update_step_mesh_size(
    change: On<ValueChange<f32>>,
    inputs: Query<&NumberInputValue, With<StepMeshSize>>,
    mut settings: ResMut<StepMeshSettings>,
    mut commands: Commands,
) {
    let Ok(displayed) = inputs.get(change.source) else {
        return;
    };
    if change.value.is_finite() && change.value >= 0.0 {
        settings.options.max_size_mm = (change.value > 0.0).then_some(change.value);
        let value = NumberInputValue::F32(change.value);
        if *displayed != value {
            commands.entity(change.source).insert(value);
        }
    }
}

fn update_step_curve_resolution(
    change: On<ValueChange<i32>>,
    inputs: Query<&NumberInputValue, With<StepCurveResolution>>,
    mut settings: ResMut<StepMeshSettings>,
    mut commands: Commands,
) {
    let Ok(displayed) = inputs.get(change.source) else {
        return;
    };
    let resolution = change.value.clamp(0, MAX_STEP_CURVE_RESOLUTION);
    settings.options.elements_per_turn = resolution as u32;
    let value = NumberInputValue::I32(resolution);
    if *displayed != value {
        commands.entity(change.source).insert(value);
    }
}

pub(super) fn spawn_step_mesh_ui(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(6),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("STEP max size (mm)\n0 = automatic limit"),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
            let row_entity = row.target_entity();
            row.commands()
                .queue_spawn_scene(bsn! {
                    @FeathersNumberInput
                    NumberInputValue::F32(0.0)
                    HardLimit(NumberInputRange::F32(0.0..=f32::MAX))
                    NumberInputPrecision(4)
                    NumberInputStep(0.1)
                    Node { width: px(86) }
                })
                .insert((
                    ChildOf(row_entity),
                    StepMeshSize,
                    Name::new("STEP maximum element size in mm"),
                ))
                .observe(update_step_mesh_size);
        });

    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(6),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("STEP curve resolution\nElements per full turn\n0 = off / 20 = default"),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
            let row_entity = row.target_entity();
            row.commands()
                .queue_spawn_scene(bsn! {
                    @FeathersNumberInput
                    NumberInputValue::I32(DEFAULT_STEP_CURVE_RESOLUTION)
                    HardLimit(NumberInputRange::I32(0..=MAX_STEP_CURVE_RESOLUTION))
                    NumberInputStep(1.0)
                    Node { width: px(86) }
                })
                .insert((
                    ChildOf(row_entity),
                    StepCurveResolution,
                    Name::new("STEP elements per turn"),
                ))
                .observe(update_step_curve_resolution);
        });

    spawn_step_import_ui(parent);

    parent
        .spawn((
            SaveStepMeshControls,
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                ..default()
            },
        ))
        .with_children(|controls| {
            controls
                .spawn((
                    InteractionButton,
                    SaveStepMeshButton,
                    Node {
                        padding: UiRect::axes(px(8), px(4)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(5)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(PANEL_BORDER),
                ))
                .with_child((
                    Text::new("Save FrontISTR Mesh (.msh)..."),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));
            controls.spawn((
                SaveStepMeshStatus,
                Text::default(),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
        });
}

pub(crate) fn save_step_mesh_button_system(
    model: Option<Res<FemModel>>,
    step: Res<StepImport>,
    editor: Res<AssemblyEditorState>,
    mut controls: Query<&mut Node, With<SaveStepMeshControls>>,
    mut buttons: Query<(Ref<ButtonInteraction>, &mut BackgroundColor), With<SaveStepMeshButton>>,
    mut status: Query<&mut Text, With<SaveStepMeshStatus>>,
) {
    let Some((part, mesh)) = model.as_deref().and_then(|model| {
        if step.is_active() {
            return None;
        }
        let part = model.parts.get(editor.selected_part?)?;
        let mesh = model.meshes.get(part.mesh_index)?;
        is_step(Path::new(&part.name)).then_some((part, mesh))
    }) else {
        for mut control in &mut controls {
            if control.display != Display::None {
                control.display = Display::None;
            }
        }
        for mut text in &mut status {
            if !text.is_empty() {
                text.clear();
            }
        }
        return;
    };
    for mut control in &mut controls {
        if control.display != Display::Flex {
            control.display = Display::Flex;
        }
    }
    for (interaction, mut background) in &mut buttons {
        background.set_if_neq(BackgroundColor(match *interaction {
            ButtonInteraction::Pressed => BUTTON_PRESSED,
            ButtonInteraction::Hovered => BUTTON_HOVERED,
            ButtonInteraction::None => BUTTON_NORMAL,
        }));
        if *interaction != ButtonInteraction::Pressed || !interaction.is_changed() {
            continue;
        }
        let name = Path::new(&part.name).with_extension("msh");
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save FrontISTR mesh geometry and groups")
            .add_filter("FrontISTR mesh (.msh)", &["msh"])
            .set_file_name(name.to_string_lossy())
            .save_file()
        else {
            continue;
        };
        let message = if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("msh"))
        {
            match hecmw::write_msh_geometry_file(&path, mesh) {
                Ok((nodes, elements)) => {
                    let file = path.file_name().unwrap_or_default().to_string_lossy();
                    format!("Saved {file} ({nodes} nodes, {elements} elements)")
                }
                Err(error) => format!("Could not save mesh: {error}"),
            }
        } else {
            "Enter a filename ending in .msh.".into()
        };
        for mut text in &mut status {
            **text = message.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_controls_update_job_options_and_displayed_values() {
        let mut app = App::new();
        app.init_resource::<StepMeshSettings>();
        let world = app.world_mut();
        let max_size = world
            .spawn((StepMeshSize, NumberInputValue::F32(0.0)))
            .observe(update_step_mesh_size)
            .id();
        let resolution = world
            .spawn((
                StepCurveResolution,
                NumberInputValue::I32(DEFAULT_STEP_CURVE_RESOLUTION),
            ))
            .observe(update_step_curve_resolution)
            .id();

        world.trigger(ValueChange {
            source: max_size,
            value: 4.0_f32,
            is_final: true,
        });
        world.trigger(ValueChange {
            source: resolution,
            value: 40_i32,
            is_final: true,
        });
        world.flush();
        assert_eq!(world.get(max_size), Some(&NumberInputValue::F32(4.0)));
        assert_eq!(world.get(resolution), Some(&NumberInputValue::I32(40)));
        assert_eq!(
            world.resource::<StepMeshSettings>().options,
            StepMeshOptions {
                max_size_mm: Some(4.0),
                elements_per_turn: 40,
            }
        );

        world.trigger(ValueChange {
            source: max_size,
            value: 0.0_f32,
            is_final: true,
        });
        world.trigger(ValueChange {
            source: resolution,
            value: 0_i32,
            is_final: true,
        });
        world.flush();
        assert_eq!(
            world.resource::<StepMeshSettings>().options,
            StepMeshOptions {
                max_size_mm: None,
                elements_per_turn: 0,
            }
        );
    }
}
