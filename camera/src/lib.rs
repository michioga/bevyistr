mod focus;
mod navigation_cube;
mod orbit;

use bevy::prelude::*;
use interaction::InteractionSystems;

use focus::focus_selected_system;
use navigation_cube::{
    NavigationCubeState, animate_navigation_cube_view, navigation_cube_input_system,
    spawn_navigation_cube, sync_navigation_cube_camera, update_navigation_cube_highlight,
    update_navigation_cube_viewport,
};
pub use orbit::*;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<fem_core::UiPointerState>();
        app.init_resource::<fem_core::UiKeyboardState>();
        app.init_resource::<NavigationCubeState>();
        app.add_systems(Startup, spawn_navigation_cube);
        app.add_systems(
            Update,
            (
                orbit_camera_system,
                focus_selected_system,
                navigation_cube_input_system,
                animate_navigation_cube_view
                    .after(orbit_camera_system)
                    .after(navigation_cube_input_system),
                sync_navigation_cube_camera.after(animate_navigation_cube_view),
                update_navigation_cube_viewport,
                update_navigation_cube_highlight.after(navigation_cube_input_system),
            )
                .in_set(InteractionSystems::Navigation),
        );
    }
}
