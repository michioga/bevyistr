//! Explicit view-only color range choice, independent of deformation.
use bevy::{
    prelude::*,
    ui::InteractionDisabled,
    ui_widgets::{Activate, Button as WidgetButton},
};
use fem_core::FemResultSet;
use visualization::ContourRangeMode;

#[derive(Component)]
struct RangeButton(ContourRangeMode);

pub(crate) fn register(app: &mut App) {
    app.init_resource::<ContourRangeMode>()
        .add_systems(Update, sync);
}

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    let text = |value: &str| {
        (
            Text::new(value),
            Pickable::IGNORE,
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(Color::WHITE),
        )
    };
    parent.spawn(text("COLOR RANGE | compare frames"));
    parent
        .spawn(Node {
            width: percent(100),
            column_gap: px(4),
            ..default()
        })
        .with_children(|row| {
            for (mode, label) in [
                (ContourRangeMode::CurrentFrame, "Current frame"),
                (ContourRangeMode::AllFrames, "All frames"),
            ] {
                row.spawn((
                    Button,
                    WidgetButton,
                    RangeButton(mode),
                    bevy::input_focus::tab_navigation::TabIndex(0),
                    Node {
                        flex_grow: 1.0,
                        flex_basis: px(0),
                        min_height: px(28),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(4)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.10, 0.14, 0.17)),
                    BorderColor::all(Color::srgb(0.35, 0.55, 0.62)),
                ))
                .observe(choose)
                .with_child(text(label));
            }
        });
    parent.spawn(text("All frames: same color = same value during playback."));
}

fn choose(
    event: On<Activate>,
    buttons: Query<&RangeButton>,
    results: Res<FemResultSet>,
    mut mode: ResMut<ContourRangeMode>,
) {
    if results.active_field().is_some() {
        if let Ok(button) = buttons.get(event.entity) {
            mode.set_if_neq(button.0);
        }
    }
}

fn sync(
    mut commands: Commands,
    results: Res<FemResultSet>,
    mode: Res<ContourRangeMode>,
    mut buttons: Query<(Entity, &RangeButton, &mut BackgroundColor)>,
) {
    for (entity, button, mut color) in &mut buttons {
        let enabled = results.active_field().is_some();
        if enabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
        color.set_if_neq(BackgroundColor(if !enabled {
            Color::srgb(0.10, 0.12, 0.14)
        } else if button.0 == *mode {
            Color::srgb(0.18, 0.45, 0.55)
        } else {
            Color::srgb(0.10, 0.14, 0.17)
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_choice_changes_only_view_policy_and_is_disabled_without_a_field() {
        let mut app = App::new();
        app.add_plugins(bevy::ui_widgets::ButtonPlugin)
            .init_resource::<FemResultSet>()
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(spawn);
            });
        register(&mut app);
        app.update();
        let button = app
            .world_mut()
            .query::<(Entity, &RangeButton)>()
            .iter(app.world())
            .find(|(_, b)| b.0 == ContourRangeMode::AllFrames)
            .unwrap()
            .0;
        assert!(app.world().get::<InteractionDisabled>(button).is_some());
        app.world_mut().resource_mut::<FemResultSet>().by_mesh = vec![vec![fem_core::StepResult {
            fields: vec![fem_core::ResultField::NodeScalar {
                name: "P".into(),
                values: vec![1.],
                min: 1.,
                max: 1.,
            }],
            ..default()
        }]];
        app.world_mut()
            .resource_mut::<FemResultSet>()
            .activate_first();
        app.update();
        crate::widget_test_input::click(app.world_mut(), button);
        assert_eq!(
            *app.world().resource::<ContourRangeMode>(),
            ContourRangeMode::AllFrames
        );
        assert_eq!(
            app.world()
                .resource::<FemResultSet>()
                .active_field()
                .unwrap()
                .constant_value(),
            Some(1.)
        );
    }
}
