mod components;
mod selection_system;
mod state;

use bevy::prelude::*;
use interaction::InteractionSystems;

pub use components::*;
pub use selection_system::{
    clear_selection_shortcut_system, click_selection_system, selection_filter_shortcut_system,
};
pub use state::*;

pub struct SelectionPlugin;

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<fem_core::SelectionFilter>();
        app.init_resource::<fem_core::UiPointerState>();
        app.init_resource::<fem_core::UiKeyboardState>();
        app.init_resource::<fem_core::ViewportTool>();
        app.init_resource::<SelectionState>();
        app.init_resource::<ClickSequence>();

        app.add_systems(
            Update,
            (
                selection_filter_shortcut_system,
                clear_selection_shortcut_system,
            )
                .in_set(InteractionSystems::UiInput),
        );
        app.add_systems(
            Update,
            click_selection_system.in_set(InteractionSystems::Selection),
        );
    }
}
