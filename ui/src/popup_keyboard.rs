//! Keyboard handling shared by popup items. Keep multi-select activation
//! distinct from explicit cancellation; native MenuPlugin handles Enter.
use bevy::{
    input::{ButtonState, keyboard::KeyboardInput},
    input_focus::{
        FocusCause, FocusedInput, InputFocus,
        tab_navigation::{NavAction, TabNavigation},
    },
    prelude::*,
    ui::ScrollPosition,
    ui_widgets::{MenuAction, MenuEvent, MenuPopup},
};

pub(crate) fn handle(
    mut event: On<FocusedInput<KeyboardInput>>,
    parents: Query<&ChildOf>,
    popups: Query<(), With<MenuPopup>>,
    navigation: TabNavigation,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
    layout: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut scrolls: Query<&mut ScrollPosition>,
) {
    if event.input.repeat || event.input.state != ButtonState::Pressed {
        return;
    }
    let Some(popup) = parents
        .iter_ancestors(event.original_event_target())
        .find(|e| popups.contains(*e))
    else {
        return;
    };
    let action = match event.input.key_code {
        KeyCode::Escape => {
            event.propagate(false);
            // The source must be the popup, not a multi-select item: item
            // activation deliberately suppresses dismissal in output menus.
            commands.trigger(MenuEvent {
                source: popup,
                action: MenuAction::FocusRoot,
            });
            commands.trigger(MenuEvent {
                source: popup,
                action: MenuAction::CloseAll,
            });
            return;
        }
        KeyCode::ArrowUp => NavAction::Previous,
        KeyCode::ArrowDown => NavAction::Next,
        KeyCode::Home => NavAction::First,
        KeyCode::End => NavAction::Last,
        _ => return,
    };
    event.propagate(false);
    if let Ok(next) = navigation.navigate(&focus, action) {
        focus.set(next, FocusCause::Navigated);
        // Layout coordinates are physical pixels; ScrollPosition uses logical
        // pixels. Reveal the target without scrolling the surrounding sidebar.
        if let (Ok((item, item_transform)), Ok((menu, menu_transform)), Ok(mut scroll)) =
            (layout.get(next), layout.get(popup), scrolls.get_mut(popup))
        {
            let scale = menu.inverse_scale_factor;
            let height = menu.size().y * scale;
            if height > 8.0 && item.size().y > 0.0 {
                let top = menu_transform.transform_point2(Vec2::ZERO).y * scale - height * 0.5;
                let item_height = item.size().y * item.inverse_scale_factor;
                let item_top = item_transform.transform_point2(Vec2::ZERO).y
                    * item.inverse_scale_factor
                    - item_height * 0.5;
                let delta = reveal_delta(
                    item_top,
                    item_top + item_height,
                    top + 4.0,
                    top + height - 4.0,
                );
                let max_scroll = ((menu.content_size().y - menu.size().y) * scale).max(0.0);
                scroll.0.y = (scroll.0.y + delta).clamp(0.0, max_scroll);
            }
        }
    }
}

fn reveal_delta(item_top: f32, item_bottom: f32, view_top: f32, view_bottom: f32) -> f32 {
    if item_top < view_top {
        item_top - view_top
    } else if item_bottom > view_bottom {
        item_bottom - view_bottom
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reveal_moves_only_as_far_as_needed() {
        assert_eq!(reveal_delta(30.0, 50.0, 10.0, 90.0), 0.0);
        assert_eq!(reveal_delta(0.0, 20.0, 10.0, 90.0), -10.0);
        assert_eq!(reveal_delta(80.0, 100.0, 10.0, 90.0), 10.0);
    }
}
