//! Shared visual affordance for dropdowns; only the parent is interactive.
use bevy::{
    input_focus::InputFocus, picking::hover::Hovered, prelude::*, ui::InteractionDisabled,
    ui_widgets::MenuPopup,
};

#[derive(Component)]
pub(crate) struct Trigger;
#[derive(Component)]
struct Arrow;
#[derive(Component)]
struct Hint;
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MenuSync;

pub(crate) fn register(app: &mut App) {
    app.add_systems(Update, sync.after(MenuSync));
}

pub(crate) fn bundle() -> impl Bundle {
    (
        Trigger,
        Hovered::default(),
        BorderColor::all(Color::srgb(0.35, 0.55, 0.62)),
    )
}

pub(crate) fn node() -> Node {
    Node {
        width: percent(100),
        min_height: px(32),
        padding: UiRect::all(px(5)),
        border: UiRect::all(px(1)),
        align_items: AlignItems::Center,
        column_gap: px(6),
        border_radius: BorderRadius::all(px(4)),
        ..default()
    }
}

pub(crate) fn content(parent: &mut ChildSpawnerCommands, label: impl Bundle, hint: &'static str) {
    parent.spawn((
        label,
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            ..default()
        },
    ));
    parent.spawn((
        Arrow,
        Text::new(">"),
        Pickable::IGNORE,
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            width: px(24),
            flex_shrink: 0.0,
            padding: UiRect::left(px(8)),
            border: UiRect::left(px(1)),
            ..default()
        },
        BorderColor::all(Color::srgb(0.35, 0.55, 0.62)),
    ));
    parent.spawn((
        Hint,
        Text::new(hint),
        Pickable::IGNORE,
        OverrideClip,
        GlobalZIndex(210),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(Color::WHITE),
        BackgroundColor(Color::srgb(0.07, 0.10, 0.12)),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            top: percent(100),
            left: px(0),
            width: percent(100),
            padding: UiRect::all(px(6)),
            ..default()
        },
    ));
}

fn sync(
    mut triggers: Query<
        (
            Entity,
            &ChildOf,
            &Hovered,
            Option<&InteractionDisabled>,
            &mut BorderColor,
        ),
        With<Trigger>,
    >,
    parents: Query<&Children>,
    popups: Query<(), With<MenuPopup>>,
    focus: Res<InputFocus>,
    mut arrows: Query<(&ChildOf, &mut Text), With<Arrow>>,
    mut hints: Query<(&ChildOf, &mut Node), With<Hint>>,
) {
    for (entity, parent, hovered, disabled, mut border) in &mut triggers {
        let open = parents
            .get(parent.parent())
            .is_ok_and(|children| children.iter().any(|e| popups.contains(e)));
        let hot = disabled.is_none() && (hovered.get() || focus.get() == Some(entity));
        border.set_if_neq(BorderColor::all(if disabled.is_some() {
            Color::srgb(0.2, 0.25, 0.27)
        } else if hot || open {
            Color::srgb(0.55, 0.85, 0.95)
        } else {
            Color::srgb(0.35, 0.55, 0.62)
        }));
        for (parent, mut text) in &mut arrows {
            if parent.parent() == entity {
                text.set_if_neq(Text::new(if open { "v" } else { ">" }));
            }
        }
        for (parent, mut node) in &mut hints {
            if parent.parent() == entity {
                node.display = if hot && !open {
                    Display::Flex
                } else {
                    Display::None
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arrow_hint_and_border_follow_open_hover_disabled_and_closed_states() {
        let mut app = App::new();
        app.init_resource::<InputFocus>();
        register(&mut app);
        let anchor = app.world_mut().spawn(Node::default()).id();
        let button = app
            .world_mut()
            .spawn((ChildOf(anchor), node(), bundle()))
            .id();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        Commands::new(&mut queue, app.world())
            .entity(button)
            .with_children(|p| {
                content(
                    p,
                    (
                        Text::new("Display field: Long custom quantity name"),
                        Pickable::IGNORE,
                    ),
                    "Choose field",
                )
            });
        queue.apply(app.world_mut());
        let children = app.world().get::<Children>(button).unwrap().to_vec();
        let arrow = children[1];
        let hint = children[2];
        app.update();
        assert_eq!(app.world().get::<Text>(arrow).unwrap().0, ">");
        let normal = *app.world().get::<BorderColor>(button).unwrap();
        app.world_mut().entity_mut(button).insert(Hovered(true));
        app.update();
        assert_eq!(
            app.world().get::<Node>(hint).unwrap().display,
            Display::Flex
        );
        assert_ne!(*app.world().get::<BorderColor>(button).unwrap(), normal);
        for child in &children {
            assert_eq!(app.world().get::<Pickable>(*child), Some(&Pickable::IGNORE));
        }
        let popup = app
            .world_mut()
            .spawn((ChildOf(anchor), MenuPopup::default()))
            .id();
        app.update();
        assert_eq!(app.world().get::<Text>(arrow).unwrap().0, "v");
        assert_eq!(
            app.world().get::<Node>(hint).unwrap().display,
            Display::None
        );
        app.world_mut().despawn(popup);
        app.world_mut()
            .entity_mut(button)
            .insert(InteractionDisabled);
        app.update();
        assert_eq!(app.world().get::<Text>(arrow).unwrap().0, ">");
        assert_eq!(
            app.world().get::<Node>(hint).unwrap().display,
            Display::None
        );
    }
}
