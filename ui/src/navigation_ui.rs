use crate::{ButtonInteraction, InteractionButton};
use bevy::prelude::*;
use camera::NavigationScheme;

use crate::app_settings::AppSettings;

#[derive(Component)]
pub(crate) struct NavigationButton(NavigationScheme);

#[derive(Component)]
pub(crate) struct NavigationHelp;

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            column_gap: px(4),
            ..default()
        })
        .with_children(|row| {
            for (scheme, label) in [
                (NavigationScheme::Mouse, "Mouse"),
                (NavigationScheme::TouchpadAlt, "Touchpad (Alt)"),
                (NavigationScheme::TouchpadSpace, "Touchpad (Space)"),
            ] {
                row.spawn((
                    InteractionButton,
                    NavigationButton(scheme),
                    Node {
                        flex_grow: 1.0,
                        height: px(23),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(4)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.10, 0.12, 0.14)),
                    BorderColor::all(Color::srgb(0.30, 0.38, 0.43)),
                ))
                .with_child((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(Color::srgb(0.88, 0.92, 0.94)),
                ));
            }
        });
    parent.spawn((
        NavigationHelp,
        Text::default(),
        TextFont {
            font_size: FontSize::Px(9.5),
            ..default()
        },
        TextColor(Color::srgb(0.58, 0.66, 0.70)),
    ));
}

pub(crate) fn update(
    mut scheme: ResMut<NavigationScheme>,
    settings: Res<AppSettings>,
    mut buttons: Query<(&NavigationButton, Ref<ButtonInteraction>, &mut BackgroundColor)>,
    mut help: Query<&mut Text, With<NavigationHelp>>,
) {
    for (option, interaction, _) in &mut buttons {
        if interaction.is_changed() && *interaction == ButtonInteraction::Pressed {
            scheme.set_if_neq(option.0);
        }
    }
    for (option, interaction, mut color) in &mut buttons {
        let background = if option.0 == *scheme {
            Color::srgb(0.18, 0.45, 0.66)
        } else {
            crate::solver_runner::ordinary_button_color(*interaction, true)
        };
        color.set_if_neq(BackgroundColor(background));
    }
    let hint = match *scheme {
        NavigationScheme::Mouse => "Right drag: orbit | Middle drag: pan | Wheel: zoom\nShift + right/middle drag: snap orbit",
        NavigationScheme::TouchpadAlt => {
            "Alt + left drag: orbit | +Shift: pan | +Ctrl: zoom\nRemove selection: Ctrl+Shift + click/drag"
        }
        NavigationScheme::TouchpadSpace => "Space + left drag: orbit | +Shift: pan | +Ctrl: zoom",
    };
    for mut text in &mut help {
        let label = match &settings.navigation_warning {
            Some(warning) => format!("{hint}\nNavigation settings: {warning}"),
            None => hint.into(),
        };
        text.set_if_neq(Text::new(label));
    }
}
