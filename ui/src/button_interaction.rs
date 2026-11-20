use bevy::{
    ecs::entity::EntityHashMap,
    picking::{
        PickingSystems,
        events::{PointerPress, PointerState},
        hover::Hovered,
        pointer::PointerButton,
    },
    prelude::*,
    ui::InteractionDisabled,
};

#[derive(Component)]
#[require(Node, Hovered, ButtonInteraction)]
pub(crate) struct InteractionButton;

#[derive(Component, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ButtonInteraction {
    #[default]
    None,
    Hovered,
    Pressed,
}

pub(crate) fn register(app: &mut App) {
    app.add_message::<PointerPress>().add_systems(
        PreUpdate,
        sync_button_interaction.after(PickingSystems::Last),
    );
}

fn sync_button_interaction(
    mut presses: MessageReader<PointerPress>,
    pointer_state: Res<PointerState>,
    parents: Query<&ChildOf>,
    mut buttons: Query<(
        Entity,
        &Hovered,
        &InheritedVisibility,
        Has<InteractionDisabled>,
        &mut ButtonInteraction,
    )>,
) {
    // Include presses that start and end in the same frame. Held presses use
    // their original targets, so dragging onto a button does not activate it.
    let targets = presses
        .read()
        .filter(|press| press.button == PointerButton::Primary)
        .map(|press| (press.entity, true))
        .chain(
            pointer_state
                .pointer_buttons
                .iter()
                .filter(|((_, button), _)| *button == PointerButton::Primary)
                .flat_map(|(_, state)| state.pressing.keys().map(|&target| (target, false))),
        );
    let mut pressed_buttons = EntityHashMap::default();
    for (target, just_pressed) in targets {
        if let Some(button) = std::iter::once(target)
            .chain(parents.iter_ancestors(target))
            .find(|&entity| buttons.contains(entity))
        {
            pressed_buttons.entry(button).or_insert(just_pressed);
        }
    }

    for (entity, hovered, visibility, disabled, mut interaction) in &mut buttons {
        let press = pressed_buttons.get(&entity);
        let next = if !visibility.get() {
            ButtonInteraction::None
        } else if !disabled && press.is_some() {
            ButtonInteraction::Pressed
        } else if hovered.get() {
            ButtonInteraction::Hovered
        } else {
            ButtonInteraction::None
        };
        if next == ButtonInteraction::Pressed && press == Some(&true) {
            // A new click still counts if the prior press was released in the
            // same frame, without an intervening Hovered or None state.
            *interaction = next;
        } else {
            interaction.set_if_neq(next);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        camera::NormalizedRenderTarget,
        picking::{
            backend::HitData,
            events::Pointer,
            pointer::{Location, PointerId},
        },
    };

    #[test]
    fn primary_presses_reach_the_parent_button_and_only_fire_on_press() {
        let mut app = App::new();
        register(&mut app);
        #[derive(Resource, Default)]
        struct AppliedPresses(usize);

        app.init_resource::<PointerState>()
            .init_resource::<AppliedPresses>()
            .add_systems(
                Update,
                |buttons: Query<Ref<ButtonInteraction>>, mut applied: ResMut<AppliedPresses>| {
                    applied.0 += buttons
                        .iter()
                        .filter(|state| **state == ButtonInteraction::Pressed && state.is_changed())
                        .count();
                },
            );
        let button = app
            .world_mut()
            .spawn((
                InteractionButton,
                InheritedVisibility::VISIBLE,
                Hovered(true),
            ))
            .id();
        let label = app.world_mut().spawn(ChildOf(button)).id();
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 200,
                height: 100,
            },
            position: Vec2::new(30.0, 20.0),
        };
        let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
        let press = |button| PointerPress {
            entity: label,
            pointer: Pointer::new(PointerId::Mouse, location.clone()),
            button,
            hit: hit.clone(),
            count: 1,
        };
        let state = |app: &App| *app.world().get::<ButtonInteraction>(button).unwrap();

        app.world_mut()
            .write_message(press(PointerButton::Secondary));
        app.update();
        assert_eq!(state(&app), ButtonInteraction::Hovered);

        for count in 1..=2 {
            app.world_mut().write_message(press(PointerButton::Primary));
            app.update();
            assert_eq!(state(&app), ButtonInteraction::Pressed);
            assert_eq!(app.world().resource::<AppliedPresses>().0, count);
        }

        app.world_mut()
            .resource_mut::<PointerState>()
            .get_mut(PointerId::Mouse, PointerButton::Primary)
            .pressing
            .insert(label, (location, bevy::platform::time::Instant::now(), hit));
        app.update();
        assert_eq!(state(&app), ButtonInteraction::Pressed);
        assert_eq!(app.world().resource::<AppliedPresses>().0, 2);

        app.world_mut()
            .resource_mut::<PointerState>()
            .clear(PointerId::Mouse);
        app.update();
        assert_eq!(state(&app), ButtonInteraction::Hovered);
    }
}
