use bevy::{
    input::{ButtonState, keyboard::KeyboardInput},
    input_focus::{
        FocusedInput, InputFocus,
        tab_navigation::{NavAction, TabIndex},
    },
    prelude::*,
    ui_widgets::{
        ActiveDescendant, ListBox, MenuAction, MenuEvent, ReselectListRow, ScrollIntoView,
    },
};

pub(crate) fn list<'a>(
    popup: &'a mut ChildSpawnerCommands,
    nav: NavAction,
    spawn_rows: impl FnOnce(&mut ChildSpawnerCommands) -> Vec<Entity>,
) -> EntityCommands<'a> {
    let mut entity = popup.spawn((
        ListBox,
        TabIndex(0),
        Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            row_gap: px(3),
            ..default()
        },
    ));
    entity
        .observe(cancel)
        .observe(|event: On<ReselectListRow>, mut commands: Commands| {
            dismiss(event.source, &mut commands);
        })
        .with_children(|list| {
            let rows = spawn_rows(list);
            let row = match nav {
                NavAction::Last | NavAction::Previous => rows.last(),
                NavAction::First | NavAction::Next => rows.first(),
            };
            let list_entity = list.target_entity();
            list.commands()
                .entity(list_entity)
                .insert(ActiveDescendant(row.copied()));
        });
    entity
}

fn cancel(mut event: On<FocusedInput<KeyboardInput>>, mut commands: Commands) {
    let key = &event.input;
    if key.key_code == KeyCode::Escape && key.state == ButtonState::Pressed && !key.repeat {
        event.propagate(false);
        dismiss(event.focused_entity, &mut commands);
    }
}

pub(crate) fn dismiss(source: Entity, commands: &mut Commands) {
    commands.trigger(MenuEvent {
        source,
        action: MenuAction::FocusRoot,
    });
    commands.trigger(MenuEvent {
        source,
        action: MenuAction::CloseAll,
    });
}

pub(crate) fn active_row(focus: &InputFocus, lists: &Query<&ActiveDescendant>) -> Option<Entity> {
    lists.get(focus.get()?).ok()?.0
}

pub(crate) fn reveal_focused_row(
    lists: Query<&ActiveDescendant, (With<ListBox>, Changed<ActiveDescendant>)>,
    mut commands: Commands,
) {
    for entry in &lists {
        if let Some(entity) = entry.0 {
            commands.trigger(ScrollIntoView { entity });
        }
    }
}
