mod hover;

use bevy::prelude::*;

use hover::hover_system;

pub struct PickingPlugin;

impl Plugin for PickingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<fem_core::SelectionFilter>();
        app.init_resource::<fem_core::UiPointerState>();

        app.add_systems(Update, hover_system);
    }
}
