mod focus;
mod orbit;

use bevy::prelude::*;
use interaction::InteractionSystems;

use focus::focus_selected_system;
pub use orbit::*;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<fem_core::UiPointerState>();
        app.add_systems(
            Update,
            (orbit_camera_system, focus_selected_system).in_set(InteractionSystems::Navigation),
        );
    }
}
