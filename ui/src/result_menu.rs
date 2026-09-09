//! Result-driven popup menu, following the supplied bevy_menu example.
use crate::layout::{ScrollableList, SidebarPage, UiInputCapture};
use bevy::{
    input_focus::{
        FocusCause, InputFocus,
        tab_navigation::{NavAction, TabIndex},
    },
    picking::hover::Hovered,
    prelude::*,
    ui::{InteractionDisabled, ScrollPosition},
    ui_widgets::{
        Activate, MenuAction, MenuButton, MenuEvent, MenuFocusState, MenuItem, MenuPopup,
        popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
    },
};
use fem_core::{ActiveResult, FemResultSet, ResultField};
use visualization::{ContourSettings, VisualizationSettings};

#[derive(Component)]
struct FieldMenu;
#[derive(Component)]
struct FieldButton;
#[derive(Component)]
struct FieldLabel;
#[derive(Component)]
struct FieldPopup;
#[derive(Component, Clone)]
struct Choice {
    name: String,
}
#[derive(Component)]
struct DeformationButton;
#[derive(Component)]
struct DeformationLabel;
#[derive(Component)]
pub(crate) struct DeformationScaleSection;

fn text(label: impl Into<String>) -> impl Bundle {
    (
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(Color::WHITE),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_fields_follow_file_order_and_missing_quantity_is_not_replaced() {
        let scalar = |name: &str| ResultField::NodeScalar {
            name: name.into(),
            values: vec![1.0],
            min: 1.0,
            max: 1.0,
        };
        let mut results = FemResultSet::default();
        results.by_mesh = vec![vec![
            fem_core::StepResult {
                step: 0,
                time: 0.0,
                fields: vec![
                    scalar("CUSTOM[2]"),
                    scalar("CUSTOM[10]"),
                    scalar("Temperature"),
                ],
            },
            fem_core::StepResult {
                step: 1,
                time: 1.0,
                fields: vec![scalar("Temperature")],
            },
        ]];
        let mut settings = VisualizationSettings::default();
        select("CUSTOM[2]", &mut results, &mut settings);
        assert_eq!(
            fields(&results)
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["CUSTOM[2]", "CUSTOM[10]", "Temperature"]
        );
        results.active.as_mut().unwrap().step_index = 1;
        settings.contour.as_mut().unwrap().step_index = 1;
        assert_eq!(fields(&results).len(), 1);
        let mut app = App::new();
        app.insert_resource(results)
            .insert_resource(settings)
            .init_resource::<InputFocus>()
            .insert_resource(SidebarPage::Results)
            .add_systems(Update, sync);
        app.update();
        let results = app.world().resource::<FemResultSet>();
        assert_eq!(results.active.as_ref().unwrap().field_name, "CUSTOM[2]");
        assert_eq!(
            app.world()
                .resource::<VisualizationSettings>()
                .contour
                .as_ref()
                .unwrap()
                .field_name,
            "CUSTOM[2]"
        );
    }

    #[test]
    fn menu_sync_system_initializes_without_query_conflicts() {
        let mut app = App::new();
        app.init_resource::<FemResultSet>()
            .init_resource::<VisualizationSettings>()
            .init_resource::<InputFocus>()
            .insert_resource(SidebarPage::Results)
            .add_systems(Update, sync);
        app.update();
    }

    #[test]
    fn file_fields_are_listed_and_selection_preserves_shape_settings() {
        let mut results = FemResultSet::default();
        results.by_mesh = vec![vec![fem_core::StepResult {
            step: 0,
            time: 0.0,
            fields: vec![
                ResultField::NodeVector {
                    name: "Displacement".into(),
                    values: vec![Vec3::X],
                    min_mag: 1.0,
                    max_mag: 1.0,
                },
                ResultField::ElementScalar {
                    name: "Custom stress (element)".into(),
                    values: vec![42.0],
                    min: 42.0,
                    max: 42.0,
                },
            ],
        }]];
        let mut settings = VisualizationSettings::default();
        assert!(select("Displacement", &mut results, &mut settings));
        settings.contour.as_mut().unwrap().deformation_scale = 17.0;
        settings.contour.as_mut().unwrap().show_deformation = true;
        assert!(select(
            "Custom stress (element)",
            &mut results,
            &mut settings
        ));
        let c = settings.contour.as_ref().unwrap();
        assert!(c.show_deformation);
        assert_eq!(c.deformation_scale, 17.0);
        assert!(displacement_available(&results, c));
        assert!(
            fields(&results)
                .iter()
                .any(|(_, label)| label == "Element: Custom stress (element)")
        );
        assert!(!select("missing", &mut results, &mut settings));
        assert_eq!(
            results.active.as_ref().unwrap().field_name,
            "Custom stress (element)"
        );
    }
}
fn button_node() -> Node {
    Node {
        width: percent(100),
        min_height: px(28),
        padding: UiRect::all(px(5)),
        align_items: AlignItems::Center,
        border_radius: BorderRadius::all(px(4)),
        ..default()
    }
}
pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            FieldMenu,
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ))
        .observe(menu_event)
        .with_children(|anchor| {
            anchor
                .spawn((
                    Button,
                    MenuButton,
                    FieldButton,
                    TabIndex(0),
                    button_node(),
                    BackgroundColor(Color::srgb(0.14, 0.30, 0.37)),
                ))
                .with_child((text("Contour: choose field  v"), FieldLabel));
        });
    parent
        .spawn((
            Button,
            DeformationButton,
            button_node(),
            BackgroundColor(Color::srgb(0.14, 0.30, 0.37)),
        ))
        .observe(toggle_deformation)
        .with_child((text("Deformation: ON"), DeformationLabel));
}

fn fields(results: &FemResultSet) -> Vec<(String, String)> {
    let step_index = results.active.as_ref().map_or(0, |a| a.step_index);
    let mut fields = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for step in results.by_mesh.iter().filter_map(|s| s.get(step_index)) {
        for field in &step.fields {
            let label = match field {
                ResultField::NodeScalar { name, .. } => format!("Node: {name}"),
                ResultField::NodeVector { name, .. } => format!("Node: {name} (magnitude)"),
                ResultField::ElementScalar { name, .. } => format!("Element: {name}"),
            };
            if seen.insert(field.name().to_string()) {
                fields.push((field.name().to_string(), label));
            }
        }
    }
    fields
}

fn menu_event(
    event: On<MenuEvent>,
    anchor: Single<(Entity, &Children), With<FieldMenu>>,
    popups: Query<Entity, With<FieldPopup>>,
    buttons: Query<Entity, With<FieldButton>>,
    results: Res<FemResultSet>,
    mut playback: ResMut<crate::results_ui::PlaybackState>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    let (anchor, children) = *anchor;
    let existing = children.iter().find(|e| popups.contains(*e));
    match event.action {
        MenuAction::CloseAll => {
            if let Some(e) = existing {
                commands.entity(e).despawn();
            }
        }
        MenuAction::FocusRoot => {
            if let Ok(e) = buttons.single() {
                focus.set(e, FocusCause::Navigated);
            }
        }
        MenuAction::Open(_) | MenuAction::Toggle => {
            if let Some(e) = existing {
                if matches!(event.action, MenuAction::Toggle) {
                    commands.entity(e).despawn();
                }
                return;
            }
            let choices = fields(&results);
            if choices.is_empty() {
                return;
            }
            playback.playing = false;
            playback.elapsed = 0.0;
            let nav = if let MenuAction::Open(nav) = event.action {
                nav
            } else {
                NavAction::First
            };
            commands
                .spawn((
                    ChildOf(anchor),
                    FieldPopup,
                    MenuPopup::default(),
                    MenuFocusState::Opening(nav),
                    GlobalZIndex(200),
                    OverrideClip,
                    UiInputCapture,
                    ScrollableList,
                    ScrollPosition::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(300),
                        max_height: Val::Vh(45.0),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        padding: UiRect::all(px(4)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.07, 0.10, 0.12)),
                    Popover {
                        positions: vec![
                            PopoverPlacement {
                                side: PopoverSide::Bottom,
                                align: PopoverAlign::Start,
                                gap: 4.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Top,
                                align: PopoverAlign::Start,
                                gap: 4.0,
                            },
                        ],
                        window_margin: 10.0,
                    },
                ))
                .with_children(|popup| {
                    for (name, label) in choices {
                        popup
                            .spawn((
                                MenuItem,
                                Choice { name },
                                Hovered::default(),
                                TabIndex(0),
                                Node {
                                    min_height: px(26),
                                    flex_shrink: 0.0,
                                    padding: UiRect::all(px(5)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.1, 0.14, 0.17)),
                            ))
                            .observe(choose_field)
                            .with_child(text(label));
                    }
                });
        }
    }
}

fn select(name: &str, results: &mut FemResultSet, settings: &mut VisualizationSettings) -> bool {
    let step_index = results.active.as_ref().map_or(0, |a| a.step_index);
    let Some(mesh_index) = results.by_mesh.iter().position(|s| {
        s.get(step_index)
            .is_some_and(|s| s.field_by_name(name).is_some())
    }) else {
        return false;
    };
    results.active = Some(ActiveResult {
        mesh_index,
        step_index,
        field_name: name.into(),
    });
    let previous = settings.contour.as_ref();
    settings.contour = Some(ContourSettings {
        mesh_index,
        step_index,
        field_name: name.into(),
        show_deformation: previous.is_some_and(|c| c.show_deformation),
        displacement_field: previous
            .map_or_else(|| "Displacement".into(), |c| c.displacement_field.clone()),
        deformation_scale: previous.map_or(1.0, |c| c.deformation_scale),
    });
    true
}
fn choose_field(
    event: On<Activate>,
    choices: Query<&Choice>,
    mut results: ResMut<FemResultSet>,
    mut settings: ResMut<VisualizationSettings>,
) {
    if let Ok(choice) = choices.get(event.entity) {
        select(&choice.name, &mut results, &mut settings);
    }
}
fn toggle_deformation(
    _: On<Activate>,
    results: Res<FemResultSet>,
    mut settings: ResMut<VisualizationSettings>,
) {
    let Some(contour) = settings.contour.as_ref() else {
        return;
    };
    if displacement_available(&results, contour) {
        settings.contour.as_mut().unwrap().show_deformation = !contour.show_deformation;
    }
}
fn displacement_available(results: &FemResultSet, contour: &ContourSettings) -> bool {
    results.by_mesh.iter().any(|steps| {
        steps.get(contour.step_index).is_some_and(|step| {
            matches!(
                step.field_by_name(&contour.displacement_field),
                Some(ResultField::NodeVector { .. })
            )
        })
    })
}

pub(crate) fn register(app: &mut App) {
    app.add_systems(
        Update,
        sync.after(crate::results_ui::apply_slider_to_results),
    );
}
fn sync(
    mut commands: Commands,
    mut results: ResMut<FemResultSet>,
    settings: ResMut<VisualizationSettings>,
    page: Res<SidebarPage>,
    mut labels: Query<&mut Text, With<FieldLabel>>,
    mut deformation_labels: Query<&mut Text, (With<DeformationLabel>, Without<FieldLabel>)>,
    buttons: Query<Entity, With<DeformationButton>>,
    popups: Query<Entity, With<FieldPopup>>,
    mut scale_sections: Query<&mut Node, With<DeformationScaleSection>>,
    mut items: Query<(&Choice, &Hovered, &mut BackgroundColor)>,
    mut focus: ResMut<InputFocus>,
    menu_focus: Query<(), Or<(With<FieldButton>, With<Choice>)>>,
) {
    let mut settings = settings;
    if *page != SidebarPage::Results && focus.get().is_some_and(|e| menu_focus.contains(e)) {
        focus.clear();
    }
    // Never silently substitute a different physical quantity on another step.
    // Another assembly part may still carry the selected quantity.
    if results.is_changed() {
        if let Some(active) = &results.active {
            if results
                .by_mesh
                .get(active.mesh_index)
                .and_then(|s| s.get(active.step_index))
                .is_none_or(|s| s.field_by_name(&active.field_name).is_none())
            {
                let name = active.field_name.clone();
                select(&name, &mut results, &mut settings);
            }
        }
    }
    if *page != SidebarPage::Results || results.is_changed() {
        for e in &popups {
            commands.entity(e).despawn();
        }
    }
    for mut label in &mut labels {
        label.set_if_neq(Text::new(results.active.as_ref().map_or_else(
            || "Contour: no result".into(),
            |a| {
                let available = results.by_mesh.iter().any(|steps| {
                    steps
                        .get(a.step_index)
                        .is_some_and(|step| step.field_by_name(&a.field_name).is_some())
                });
                if available {
                    format!("Contour: {}  v", a.field_name)
                } else {
                    format!("Contour: {} (not output at this step)  v", a.field_name)
                }
            },
        )));
    }
    let available = settings
        .contour
        .as_ref()
        .is_some_and(|c| displacement_available(&results, c));
    let enabled = available
        && settings
            .contour
            .as_ref()
            .is_some_and(|c| c.show_deformation);
    for e in &buttons {
        if available {
            commands.entity(e).remove::<InteractionDisabled>();
        } else {
            commands.entity(e).insert(InteractionDisabled);
        }
    }
    for mut label in &mut deformation_labels {
        label.set_if_neq(Text::new(if !available {
            "Deformation: unavailable"
        } else if enabled {
            "Deformation: ON"
        } else {
            "Deformation: OFF"
        }));
    }
    for mut node in &mut scale_sections {
        node.display = if enabled {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (choice, hovered, mut color) in &mut items {
        let selected = results
            .active
            .as_ref()
            .is_some_and(|a| a.field_name == choice.name);
        color.set_if_neq(BackgroundColor(if selected {
            Color::srgb(0.16, 0.43, 0.51)
        } else if hovered.get() {
            Color::srgb(0.22, 0.28, 0.32)
        } else {
            Color::srgb(0.1, 0.14, 0.17)
        }));
    }
}
