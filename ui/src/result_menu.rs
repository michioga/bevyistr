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
        Activate, Button as WidgetButton, MenuAction, MenuButton, MenuEvent, MenuFocusState,
        MenuItem, MenuPopup,
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
enum FieldLabel {
    Button,
    Current,
}
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
        // Widget observers must receive the hit on the control, not its label.
        bevy::picking::Pickable::IGNORE,
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
    fn changing_velocity_fields_never_requests_the_open_results_dialog() {
        use crate::result_open::{OpenResultRequested, ResultOpenState, request_result_open, open_result_button_system};
        use crate::results_ui::{OpenResultButton, ResultLoadError};
        let mut app = App::new();
        app.add_plugins((bevy::ui_widgets::ButtonPlugin, bevy::ui_widgets::MenuPlugin))
            .init_resource::<FemResultSet>()
            .init_resource::<VisualizationSettings>()
            .init_resource::<crate::results_ui::PlaybackState>()
            .init_resource::<crate::solve_results_ui::SolveResultsState>()
            .init_resource::<ResultOpenState>()
            .init_resource::<ResultLoadError>()
            .init_resource::<InputFocus>()
            .insert_resource(SidebarPage::Results)
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(spawn);
            })
            .add_systems(Update, (sync, open_result_button_system).chain());
        crate::widget_test_input::enable_keyboard(&mut app);
        let open = app.world_mut().spawn((Button, WidgetButton, OpenResultButton, BackgroundColor::default()))
            .observe(request_result_open).id();
        app.world_mut().resource_mut::<FemResultSet>().by_mesh = vec![vec![fem_core::StepResult {
            fields: ["VELOCITY", "VELOCITY[1]", "VELOCITY[2]"].map(|name| ResultField::NodeScalar {
                name: name.into(), values: vec![0.], min: 0., max: 0.,
            }).to_vec(), ..default()
        }]];
        app.update();
        let button = app.world_mut().query_filtered::<Entity, With<FieldButton>>().single(app.world()).unwrap();
        for name in ["VELOCITY", "VELOCITY[1]", "VELOCITY[2]", "VELOCITY[1]"] {
            crate::widget_test_input::click(app.world_mut(), button);
            app.update();
            let choice = app.world_mut().query::<(Entity, &Choice)>().iter(app.world())
                .find(|(_, c)| c.name == name).unwrap().0;
            // A popup may overlap Open Results when it opens above its anchor.
            // Simulate the stale legacy pressed state as the popup disappears.
            app.world_mut().entity_mut(open).insert(Interaction::Pressed);
            crate::widget_test_input::click(app.world_mut(), choice);
            app.update(); // Must not enter the native file dialog.
            assert!(app.world().get::<OpenResultRequested>(open).is_none());
            assert_eq!(app.world().resource::<FemResultSet>().active.as_ref().unwrap().field_name, name);
        }
        crate::widget_test_input::click(app.world_mut(), button);
        app.update();
        crate::widget_test_input::key(&mut app, KeyCode::Escape);
        assert!(app.world().get::<OpenResultRequested>(open).is_none());

        // A real click still requests a dialog; consume it without native UI.
        crate::widget_test_input::click(app.world_mut(), open);
        assert!(app.world().get::<OpenResultRequested>(open).is_some());
        app.world_mut().entity_mut(open).remove::<OpenResultRequested>();
        app.world_mut().resource_mut::<InputFocus>().set(open, FocusCause::Navigated);
        let window = app.world_mut().query_filtered::<Entity, With<bevy::window::PrimaryWindow>>().single(app.world()).unwrap();
        app.world_mut().write_message(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::Enter, logical_key: bevy::input::keyboard::Key::Enter,
                state: bevy::input::ButtonState::Pressed, text: None, repeat: false,
                window,
        });
        app.world_mut().run_schedule(PreUpdate);
        app.world_mut().flush();
        assert!(app.world().get::<OpenResultRequested>(open).is_some());
    }

    #[test]
    fn result_popup_keyboard_navigation_selects_and_escape_preserves_field() {
        let mut app = App::new();
        app.add_plugins((bevy::ui_widgets::ButtonPlugin, bevy::ui_widgets::MenuPlugin))
            .init_resource::<FemResultSet>().init_resource::<VisualizationSettings>()
            .init_resource::<crate::results_ui::PlaybackState>().init_resource::<InputFocus>()
            .insert_resource(SidebarPage::Results)
            .add_systems(Startup, |mut commands: Commands| { commands.spawn(Node::default()).with_children(spawn); })
            .add_systems(Update, sync);
        crate::widget_test_input::enable_keyboard(&mut app);
        app.world_mut().resource_mut::<FemResultSet>().by_mesh = vec![vec![fem_core::StepResult {
            fields: ["First", "Second"].map(|name| ResultField::NodeScalar {
                name: name.into(), values: vec![1.0], min: 1.0, max: 1.0,
            }).to_vec(), ..default()
        }]];
        app.update();
        let button = app.world_mut().query_filtered::<Entity, With<FieldButton>>().single(app.world()).unwrap();
        crate::widget_test_input::click(app.world_mut(), button); app.update();
        let popup = app.world_mut().query_filtered::<Entity, With<FieldPopup>>().single(app.world()).unwrap();
        crate::widget_test_input::key(&mut app, KeyCode::End);
        let last = app.world().resource::<InputFocus>().get().unwrap();
        assert_eq!(app.world().get::<Choice>(last).unwrap().name, "Second");
        crate::widget_test_input::key(&mut app, KeyCode::Home);
        let first = app.world().resource::<InputFocus>().get().unwrap();
        assert_eq!(app.world().get::<Choice>(first).unwrap().name, "First");
        crate::widget_test_input::key(&mut app, KeyCode::ArrowDown);
        let focused = app.world().resource::<InputFocus>().get().unwrap();
        assert_eq!(app.world().get::<Choice>(focused).unwrap().name, "Second");
        crate::widget_test_input::key(&mut app, KeyCode::Enter);
        assert!(app.world().get_entity(popup).is_err());
        assert_eq!(app.world().resource::<VisualizationSettings>().contour.as_ref().unwrap().field_name, "Second");
        crate::widget_test_input::click(app.world_mut(), button); app.update();
        let popup = app.world_mut().query_filtered::<Entity, With<FieldPopup>>().single(app.world()).unwrap();
        crate::widget_test_input::key(&mut app, KeyCode::Escape);
        assert!(app.world().get_entity(popup).is_err());
        assert_eq!(app.world().resource::<VisualizationSettings>().contour.as_ref().unwrap().field_name, "Second");
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(button));
    }

    #[test]
    fn popup_lists_loaded_fields_and_activation_changes_only_the_color_field() {
        let mut app = App::new();
        app.add_plugins((bevy::ui_widgets::ButtonPlugin, bevy::ui_widgets::MenuPlugin));
        app.init_resource::<FemResultSet>()
            .init_resource::<VisualizationSettings>()
            .init_resource::<crate::results_ui::PlaybackState>()
            .init_resource::<InputFocus>()
            .insert_resource(SidebarPage::Results)
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(spawn);
            })
            .add_systems(Update, sync);
        app.update();
        let button = app
            .world_mut()
            .query_filtered::<Entity, With<FieldButton>>()
            .single(app.world())
            .unwrap();
        assert!(app.world().get::<InteractionDisabled>(button).is_some());
        app.world_mut().resource_mut::<FemResultSet>().by_mesh = vec![vec![fem_core::StepResult {
            step: 7,
            time: 1.5,
            fields: vec![
                ResultField::NodeVector {
                    name: "Displacement".into(),
                    values: vec![Vec3::X],
                    min_mag: 1.0,
                    max_mag: 1.0,
                },
                ResultField::ElementScalar {
                    name: "Custom flux (element)".into(),
                    values: vec![42.0],
                    min: 42.0,
                    max: 42.0,
                },
            ],
        }]];
        app.world_mut().resource_mut::<FemResultSet>().active = Some(ActiveResult {
            mesh_index: 0,
            step_index: 0,
            field_name: "Displacement".into(),
        });
        app.world_mut()
            .resource_mut::<VisualizationSettings>()
            .contour = Some(ContourSettings {
            mesh_index: 0,
            step_index: 0,
            field_name: "Displacement".into(),
            show_deformation: true,
            displacement_field: "Displacement".into(),
            deformation_scale: 17.0,
        });
        app.update();
        assert!(app.world().get::<InteractionDisabled>(button).is_none());
        let button_text = app.world().get::<Children>(button).unwrap()[0];
        assert_eq!(
            app.world().get::<bevy::picking::Pickable>(button_text),
            Some(&bevy::picking::Pickable::IGNORE)
        );
        crate::widget_test_input::click(app.world_mut(), button);
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<FieldPopup>>()
                .iter(app.world())
                .count(),
            1,
            "pointer must open before the focus update"
        );
        app.update();
        let choices = app
            .world_mut()
            .query::<(Entity, &Choice)>()
            .iter(app.world())
            .map(|(entity, choice)| (entity, choice.name.clone()))
            .collect::<Vec<_>>();
        assert_eq!(choices.len(), 2);
        assert!(
            choices
                .iter()
                .all(|(_, name)| name == "Displacement" || name == "Custom flux (element)")
        );
        let selected = choices
            .iter()
            .find(|(_, name)| name == "Custom flux (element)")
            .unwrap()
            .0;
        let item_text = app.world().get::<Children>(selected).unwrap()[0];
        assert_eq!(
            app.world().get::<bevy::picking::Pickable>(item_text),
            Some(&bevy::picking::Pickable::IGNORE)
        );
        crate::widget_test_input::click(app.world_mut(), selected);
        let contour = app
            .world()
            .resource::<VisualizationSettings>()
            .contour
            .as_ref()
            .unwrap();
        assert_eq!(contour.field_name, "Custom flux (element)");
        assert!(contour.show_deformation);
        assert_eq!(contour.deformation_scale, 17.0);
        assert_eq!(contour.step_index, 0);
        assert_eq!(
            app.world()
                .resource::<FemResultSet>()
                .active
                .as_ref()
                .unwrap()
                .field_name,
            contour.field_name
        );
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<FieldPopup>>()
                .iter(app.world())
                .count(),
            0
        );
        let deformation = app
            .world_mut()
            .query_filtered::<Entity, With<DeformationButton>>()
            .single(app.world())
            .unwrap();
        let deformation_text = app.world().get::<Children>(deformation).unwrap()[0];
        assert_eq!(
            app.world().get::<bevy::picking::Pickable>(deformation_text),
            Some(&bevy::picking::Pickable::IGNORE)
        );
        crate::widget_test_input::click(app.world_mut(), deformation);
        let contour = app
            .world()
            .resource::<VisualizationSettings>()
            .contour
            .as_ref()
            .unwrap();
        assert!(!contour.show_deformation);
        assert_eq!(contour.field_name, "Custom flux (element)");
    }

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
    parent.spawn(text("CONTOUR | fields in the loaded result"));
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
                    // The UI marker alone does not emit Activate in Bevy 0.19.
                    WidgetButton,
                    MenuButton,
                    FieldButton,
                    TabIndex(0),
                    crate::popup_trigger::node(),
                    crate::popup_trigger::bundle(),
                    BackgroundColor(Color::srgb(0.14, 0.30, 0.37)),
                ))
                .with_children(|button| crate::popup_trigger::content(button,
                    (text("Display field: none"), FieldLabel::Button),
                    "Click to choose a contour field. Enter opens; Esc closes."));
        });
    parent.spawn((text("Contour: no result"), FieldLabel::Current));
    parent.spawn(text(
        "Choose the color field above. Hover the model for values (pause playback first). Deformation changes shape independently.",
    ));
    crate::result_range_ui::spawn(parent);
    parent
        .spawn((
            Button,
            WidgetButton,
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
                        max_width: Val::Vw(90.0),
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
                    popup.spawn(text(format!(
                        "{} fields in this result frame",
                        choices.len()
                    )));
                    popup.spawn(text("Click a field to change the contour. Esc cancels."));
                    for (name, label) in choices {
                        let selected = results
                            .active
                            .as_ref()
                            .is_some_and(|a| a.field_name == name);
                        let label = format!("{} {label}", if selected { "[x]" } else { "[ ]" });
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
                            .observe(crate::popup_keyboard::handle)
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
        sync.after(crate::results_ui::apply_slider_to_results).in_set(crate::popup_trigger::MenuSync),
    );
}
fn sync(
    mut commands: Commands,
    mut results: ResMut<FemResultSet>,
    settings: ResMut<VisualizationSettings>,
    page: Res<SidebarPage>,
    mut labels: Query<(&FieldLabel, &mut Text)>,
    mut deformation_labels: Query<&mut Text, (With<DeformationLabel>, Without<FieldLabel>)>,
    buttons: Query<Entity, With<DeformationButton>>,
    field_buttons: Query<Entity, With<FieldButton>>,
    popups: Query<Entity, With<FieldPopup>>,
    mut scale_sections: Query<&mut Node, With<DeformationScaleSection>>,
    mut items: Query<(Entity, &Choice, &Hovered, &mut BackgroundColor)>,
    mut focus: ResMut<InputFocus>,
    menu_focus: Query<(), Or<(With<FieldButton>, With<Choice>)>>,
    mut field_count: Local<usize>,
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
    if results.is_changed() {
        *field_count = fields(&results).len();
    }
    for e in &field_buttons {
        if *field_count == 0 {
            commands.entity(e).insert(InteractionDisabled);
        } else {
            commands.entity(e).remove::<InteractionDisabled>();
        }
    }
    for (kind, mut label) in &mut labels {
        if matches!(kind, FieldLabel::Button) {
            label.set_if_neq(Text::new(if *field_count == 0 {
                "Display field: no results".into()
            } else {
                format!("Display field: {}", results.active.as_ref().map_or("Choose...", |a| a.field_name.as_str()))
            }));
            continue;
        }
        label.set_if_neq(Text::new(results.active.as_ref().map_or_else(
            || "Contour: no result".into(),
            |a| {
                let available = results.by_mesh.iter().any(|steps| {
                    steps
                        .get(a.step_index)
                        .is_some_and(|step| step.field_by_name(&a.field_name).is_some())
                });
                if available {
                    format!("{} fields available | click above to choose", *field_count)
                } else {
                    format!("Contour: {} (not output at this step)", a.field_name)
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
    for (entity, choice, hovered, mut color) in &mut items {
        let selected = results
            .active
            .as_ref()
            .is_some_and(|a| a.field_name == choice.name);
        color.set_if_neq(BackgroundColor(if focus.get() == Some(entity) {
            Color::srgb(0.22, 0.40, 0.47)
        } else if selected {
            Color::srgb(0.16, 0.43, 0.51)
        } else if hovered.get() {
            Color::srgb(0.22, 0.28, 0.32)
        } else {
            Color::srgb(0.1, 0.14, 0.17)
        }));
    }
}
