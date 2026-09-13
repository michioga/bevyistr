//! Draft-based multi-select popup for project output controls, not contours.
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
use fem_core::{AnalysisSetup, OUTPUT_QUANTITIES, OutputControl, OutputSettings, OutputTarget};

#[derive(Component)]
struct OutputMenu(OutputTarget);
#[derive(Component)]
struct OutputLabel(OutputTarget);
#[derive(Component)]
struct Draft {
    target: OutputTarget,
    original: OutputSettings,
    control: OutputControl,
}
#[derive(Component)]
struct Choice {
    keyword: &'static str,
    value: Option<bool>,
    popup: Entity,
}
#[derive(Component)]
struct Apply(Entity);

fn text(value: impl Into<String>) -> impl Bundle {
    (
        Text::new(value),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(Color::WHITE),
    )
}

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent.spawn(text("OUTPUT FIELDS | next analysis"));
    for target in [OutputTarget::Res, OutputTarget::Vis] {
        parent
            .spawn((
                OutputMenu(target),
                Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
            ))
            .observe(menu_event)
            .with_children(|menu| {
                menu.spawn((
                    Button,
                    MenuButton,
                    TabIndex(0),
                    Node {
                        width: percent(100),
                        min_height: px(28),
                        padding: UiRect::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.14, 0.30, 0.37)),
                ))
                .with_child((
                    text(format!("{} fields...", target.label())),
                    OutputLabel(target),
                ));
            });
    }
    parent.spawn(text(
        "DISP + NMISES: basics. Output is separate from Results / Contour.",
    ));
}

fn menu_event(
    event: On<MenuEvent>,
    menus: Query<(Entity, &OutputMenu, &Children)>,
    parents: Query<&ChildOf>,
    drafts: Query<&Draft>,
    choices: Query<&Choice>,
    buttons: Query<(), With<MenuButton>>,
    setup: Res<AnalysisSetup>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    // A quantity choice must keep the popup and keyboard focus open. Apply,
    // Escape, outside click and focus loss retain native menu dismissal.
    if choices.contains(event.source)
        && matches!(event.action, MenuAction::CloseAll | MenuAction::FocusRoot)
    {
        return;
    }
    let Some((anchor, menu, children)) = std::iter::once(event.source)
        .chain(parents.iter_ancestors(event.source))
        .find_map(|e| menus.get(e).ok())
    else {
        return;
    };
    let existing = children.iter().find(|e| drafts.contains(*e));
    match event.action {
        MenuAction::FocusRoot => {
            if let Some(button) = children.iter().find(|e| buttons.contains(*e)) {
                focus.set(button, FocusCause::Navigated);
            }
        }
        MenuAction::CloseAll => {
            if let Some(e) = existing {
                commands.entity(e).despawn();
            }
        }
        MenuAction::Open(_) | MenuAction::Toggle => {
            if let Some(e) = existing {
                if matches!(event.action, MenuAction::Toggle) {
                    commands.entity(e).despawn();
                }
                return;
            }
            let target = menu.0;
            let control = setup.output.get(target).clone();
            let editable = control.editable(target);
            let nav = if let MenuAction::Open(nav) = event.action {
                nav
            } else {
                NavAction::First
            };
            let popup = commands
                .spawn((
                    ChildOf(anchor),
                    Draft {
                        target,
                        original: setup.output.clone(),
                        control: control.clone(),
                    },
                    MenuPopup::default(),
                    MenuFocusState::Opening(nav),
                    GlobalZIndex(200),
                    OverrideClip,
                    UiInputCapture,
                    ScrollableList,
                    ScrollPosition::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(410),
                        max_width: Val::Vw(90.0),
                        max_height: Val::Vh(55.0),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        padding: UiRect::all(px(6)),
                        row_gap: px(4),
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
                .id();
            commands.entity(popup).with_children(|p| {
                p.spawn(text(format!("{} | {}", target.label(), if editable {
                    "Choose fields, then Apply. Esc cancels."
                } else { "Imported advanced controls: read-only" })));
                if editable {
                    p.spawn(text("Default = solver default, not OFF. Available fields depend on analysis / elements."));
                    // Apply at the top stays reachable without scrolling through the list.
                    p.spawn((MenuItem, Apply(popup), Hovered::default(), TabIndex(0),
                        Node { min_height: px(28), flex_shrink: 0.0, padding: UiRect::all(px(5)), ..default() },
                        BackgroundColor(Color::srgb(0.12, 0.35, 0.23))))
                        .observe(apply).with_child(text("Apply output fields"));
                    for &(keyword, label) in OUTPUT_QUANTITIES {
                        p.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center,
                            min_height: px(34), flex_shrink: 0.0, column_gap: px(3), ..default() })
                            .with_children(|row| {
                                row.spawn((text(format!("{keyword}\n{label}")), Node { flex_grow: 1.0, flex_basis: px(180), ..default() }));
                                for (value, label) in [(None, "Default"), (Some(true), "ON"), (Some(false), "OFF")] {
                                    let mut button = row.spawn((MenuItem, Choice { keyword, value, popup },
                                        Hovered::default(), TabIndex(0),
                                        Node { width: px(if value.is_none() { 52 } else { 34 }), min_height: px(27),
                                            flex_shrink: 0.0, padding: UiRect::all(px(4)), ..default() },
                                        BackgroundColor(Color::srgb(0.1, 0.14, 0.17))));
                                    // Preserve imported OFF, but don't author OFF for the two basics.
                                    if matches!(keyword, "DISP" | "NMISES") && value == Some(false) {
                                        button.insert(InteractionDisabled);
                                    }
                                    button.observe(choose).with_child(text(label));
                                }
                            });
                    }
                } else {
                    p.spawn(text("GROUP / ACTION or extended syntax is preserved on export. Edit those cards in the CNT file."));
                    for card in &control.cards {
                        p.spawn(text(format!("{}\n{}", card.header, card.lines.join("\n"))));
                    }
                    p.spawn((MenuItem, TabIndex(0), Hovered::default(),
                        Node { min_height: px(28), flex_shrink: 0.0, ..default() }))
                        .with_child(text("Close (unchanged)"));
                }
            });
        }
    }
}

fn choose(event: On<Activate>, choices: Query<&Choice>, mut drafts: Query<&mut Draft>) {
    let Ok(choice) = choices.get(event.entity) else {
        return;
    };
    if matches!(choice.keyword, "DISP" | "NMISES") && choice.value == Some(false) {
        return;
    }
    if let Ok(mut draft) = drafts.get_mut(choice.popup) {
        let target = draft.target;
        draft.control.set(target, choice.keyword, choice.value);
    }
}

fn apply(
    event: On<Activate>,
    applies: Query<&Apply>,
    drafts: Query<&Draft>,
    mut setup: ResMut<AnalysisSetup>,
) {
    let Ok(button) = applies.get(event.entity) else {
        return;
    };
    let Ok(draft) = drafts.get(button.0) else {
        return;
    };
    // A project load or undo while the menu is open must never be overwritten.
    if setup.output == draft.original && setup.output.get(draft.target) != &draft.control {
        *setup.output.get_mut(draft.target) = draft.control.clone();
    }
}

pub(crate) fn register(app: &mut App) {
    app.add_systems(
        Update,
        sync.after(crate::layout::sidebar_page_button_system),
    );
}

fn sync(
    page: Res<SidebarPage>,
    setup: Res<AnalysisSetup>,
    mut commands: Commands,
    drafts: Query<(Entity, &Draft)>,
    mut labels: Query<(&OutputLabel, &mut Text)>,
    mut choices: Query<(&Choice, &Hovered, &mut BackgroundColor)>,
) {
    for (entity, draft) in &drafts {
        if *page != SidebarPage::Solve || setup.output != draft.original {
            commands.entity(entity).despawn();
        }
    }
    for (label, mut text) in &mut labels {
        let control = setup.output.get(label.0);
        let summary = if !control.editable(label.0) {
            "imported / read-only"
        } else if ["DISP", "NMISES"]
            .iter()
            .any(|k| control.value(k) == Some(false))
        {
            "baseline OFF in CNT"
        } else {
            "choose output fields"
        };
        text.0 = format!("{}: {summary}  v", label.0.label());
    }
    for (choice, hovered, mut background) in &mut choices {
        let selected = drafts
            .get(choice.popup)
            .is_ok_and(|(_, d)| d.control.value(choice.keyword) == choice.value);
        background.set_if_neq(BackgroundColor(if selected {
            Color::srgb(0.16, 0.43, 0.51)
        } else if hovered.get() {
            Color::srgb(0.22, 0.28, 0.32)
        } else {
            Color::srgb(0.1, 0.14, 0.17)
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn popup_opens_multiple_choices_stay_open_and_dismissal_discards_draft() {
        let mut app = App::new();
        app.init_resource::<AnalysisSetup>()
            .init_resource::<InputFocus>();
        let root = app
            .world_mut()
            .spawn((OutputMenu(OutputTarget::Res), Node::default()))
            .observe(menu_event)
            .id();
        let button = app.world_mut().spawn((ChildOf(root), MenuButton)).id();
        app.world_mut().trigger(MenuEvent {
            source: button,
            action: MenuAction::Toggle,
        });
        app.world_mut().flush();
        let popup = app
            .world_mut()
            .query_filtered::<Entity, With<Draft>>()
            .single(app.world())
            .unwrap();
        let choice = app
            .world_mut()
            .query::<(Entity, &Choice)>()
            .iter(app.world())
            .find(|(_, c)| c.keyword == "REACTION" && c.value == Some(true))
            .unwrap()
            .0;
        app.world_mut().trigger(Activate { entity: choice });
        app.world_mut().trigger(MenuEvent {
            source: choice,
            action: MenuAction::FocusRoot,
        });
        app.world_mut().trigger(MenuEvent {
            source: choice,
            action: MenuAction::CloseAll,
        });
        assert!(app.world().get_entity(popup).is_ok());
        assert_eq!(
            app.world()
                .get::<Draft>(popup)
                .unwrap()
                .control
                .value("REACTION"),
            Some(true)
        );
        app.world_mut().trigger(MenuEvent {
            source: popup,
            action: MenuAction::CloseAll,
        });
        app.world_mut().flush();
        assert!(app.world().get_entity(popup).is_err());
        assert_eq!(
            app.world()
                .resource::<AnalysisSetup>()
                .output
                .res
                .value("REACTION"),
            None
        );
    }
    #[test]
    fn choices_are_drafts_until_apply_and_stale_apply_is_rejected() {
        let mut app = App::new();
        app.init_resource::<AnalysisSetup>();
        let original = OutputSettings::default();
        let popup = app
            .world_mut()
            .spawn(Draft {
                target: OutputTarget::Vis,
                control: original.vis.clone(),
                original: original.clone(),
            })
            .id();
        let choice = app
            .world_mut()
            .spawn(Choice {
                keyword: "REACTION",
                value: Some(true),
                popup,
            })
            .observe(choose)
            .id();
        let button = app.world_mut().spawn(Apply(popup)).observe(apply).id();
        app.world_mut().trigger(Activate { entity: choice });
        assert_eq!(app.world().resource::<AnalysisSetup>().output, original);
        assert_eq!(
            app.world()
                .get::<Draft>(popup)
                .unwrap()
                .control
                .value("REACTION"),
            Some(true)
        );
        app.world_mut().trigger(Activate { entity: button });
        assert_eq!(
            app.world()
                .resource::<AnalysisSetup>()
                .output
                .vis
                .value("REACTION"),
            Some(true)
        );
        assert_eq!(
            app.world()
                .resource::<AnalysisSetup>()
                .output
                .res
                .value("REACTION"),
            None
        );
        app.world_mut().resource_mut::<AnalysisSetup>().output = OutputSettings::inherited();
        app.world_mut().trigger(Activate { entity: button });
        assert_eq!(
            app.world().resource::<AnalysisSetup>().output,
            OutputSettings::inherited()
        );
    }
    #[test]
    fn sync_initializes_and_discards_stale_drafts() {
        let mut app = App::new();
        app.init_resource::<AnalysisSetup>()
            .insert_resource(SidebarPage::Solve)
            .add_systems(Update, sync);
        let original = OutputSettings::default();
        let popup = app
            .world_mut()
            .spawn(Draft {
                target: OutputTarget::Res,
                control: original.res.clone(),
                original,
            })
            .id();
        app.update();
        assert!(app.world().get_entity(popup).is_ok());
        *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Results;
        app.update();
        assert!(app.world().get_entity(popup).is_err());
    }
}
