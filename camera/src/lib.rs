mod orbit;

use bevy::prelude::*;

pub use orbit::*;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, orbit_camera_system);
    }
}
