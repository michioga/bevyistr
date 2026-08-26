mod state;

use bevy::prelude::*;

use fem_core::InteractionMode;

pub use state::*;

/// Ordered phases for every direct interaction with the 3-D viewport.
///
/// Plugins put their systems in these shared sets instead of relying on
/// plugin registration order. This keeps a pointer gesture deterministic:
/// UI capture is known before camera movement, picking observes the updated
/// camera, previews observe the current pick, and selection commits exactly
/// what was previewed.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InteractionSystems {
    UiInput,
    Navigation,
    Picking,
    Preview,
    Selection,
}

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HoverResult>();

        app.init_resource::<InteractionMode>();
        app.init_resource::<fem_core::UiKeyboardState>();

        app.configure_sets(
            Update,
            (
                InteractionSystems::UiInput,
                InteractionSystems::Navigation,
                InteractionSystems::Picking,
                InteractionSystems::Preview,
                InteractionSystems::Selection,
            )
                .chain(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct PhaseOrder(Vec<InteractionSystems>);

    fn record_ui_input(mut order: ResMut<PhaseOrder>) {
        order.0.push(InteractionSystems::UiInput);
    }

    fn record_navigation(mut order: ResMut<PhaseOrder>) {
        order.0.push(InteractionSystems::Navigation);
    }

    fn record_picking(mut order: ResMut<PhaseOrder>) {
        order.0.push(InteractionSystems::Picking);
    }

    fn record_preview(mut order: ResMut<PhaseOrder>) {
        order.0.push(InteractionSystems::Preview);
    }

    fn record_selection(mut order: ResMut<PhaseOrder>) {
        order.0.push(InteractionSystems::Selection);
    }

    #[test]
    fn interaction_phases_run_in_declared_order() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<PhaseOrder>();
        app.add_plugins(InteractionPlugin);
        app.add_systems(
            Update,
            (
                record_ui_input.in_set(InteractionSystems::UiInput),
                record_navigation.in_set(InteractionSystems::Navigation),
                record_picking.in_set(InteractionSystems::Picking),
                record_preview.in_set(InteractionSystems::Preview),
                record_selection.in_set(InteractionSystems::Selection),
            ),
        );

        app.update();

        assert_eq!(
            app.world().resource::<PhaseOrder>().0,
            vec![
                InteractionSystems::UiInput,
                InteractionSystems::Navigation,
                InteractionSystems::Picking,
                InteractionSystems::Preview,
                InteractionSystems::Selection,
            ]
        );
    }
}
