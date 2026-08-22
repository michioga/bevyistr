mod focus;
mod state;

use bevy::prelude::*;

use fem_core::InteractionMode;

pub use state::*;

use focus::*;

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HoverResult>();

        app.init_resource::<InteractionMode>();

        app.add_systems(Update, focus_selected_system);
    }
}
