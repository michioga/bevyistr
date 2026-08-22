use bevy::prelude::*;
mod rect_ui;

use rect_ui::*;

pub struct BoxSelectPlugin;

impl Plugin for BoxSelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<fem_core::SelectionFilter>();
        app.init_resource::<fem_core::UiPointerState>();
        app.init_resource::<BoxSelectState>();

        app.add_systems(Startup, spawn_selection_rect);

        app.add_systems(
            Update,
            (
                begin_box_select,
                update_box_select,
                end_box_select,
                update_rect_visual,
                perform_box_selection,
            ),
        );
    }
}

#[derive(Resource, Default)]
pub struct BoxSelectState {
    pub active: bool,

    pub start: Vec2,

    pub current: Vec2,
}
