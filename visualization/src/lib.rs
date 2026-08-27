mod boundary_viz;
mod colorbar;
mod demo_mesh;

use bevy::prelude::*;
use interaction::InteractionSystems;

use boundary_viz::{spawn_boundary_load_preview, spawn_boundary_visuals};
use colorbar::{spawn_colorbar, update_colorbar};
use demo_mesh::{
    apply_contact_review, apply_visualization_mode, respawn_elements_on_setup_change,
    respawn_visuals_on_reload, spawn_contact_candidate_highlights, spawn_demo_mesh,
    spawn_topology_highlights, update_contact_candidate_highlights, update_contact_review_pose,
    update_contour_surface, update_hover_materials, update_topology_highlights,
    update_visual_layer_visibility,
};

pub use boundary_viz::{
    BoundaryLoadPreview, BoundaryLoadPreviewArrow, BoundaryLoadPreviewKind,
    BoundaryLoadPreviewVisual, BoundaryVisual, BoundaryVisualSettings,
};
pub use colorbar::ColorbagRoot;
pub use demo_mesh::{
    ContactReviewSettings, ContourSettings, DefinedContactPreview, FemMeshVisual, FemPartVisual,
    FlatMaterial, TransparentMaterial, VisualizationMode, VisualizationSettings,
    build_part_edge_mesh, build_part_surface_mesh,
};

pub struct VisualizationPlugin;

impl Plugin for VisualizationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VisualizationSettings>();
        app.init_resource::<ContactReviewSettings>();
        app.init_resource::<DefinedContactPreview>();
        app.init_resource::<demo_mesh::ContactReviewPose>();
        app.init_resource::<fem_core::FemModelVersion>();
        app.init_resource::<fem_core::ContactCandidateState>();
        app.init_resource::<fem_core::FemResultSet>();
        app.init_resource::<fem_core::AnalysisSetup>();
        app.init_resource::<fem_core::HoverPreviewTargets>();
        app.init_resource::<BoundaryVisualSettings>();
        app.init_resource::<BoundaryLoadPreview>();
        app.add_systems(
            Startup,
            (
                spawn_demo_mesh,
                spawn_topology_highlights,
                spawn_contact_candidate_highlights,
                spawn_colorbar,
            ),
        );

        app.add_systems(
            Update,
            (
                respawn_visuals_on_reload,
                respawn_elements_on_setup_change.after(respawn_visuals_on_reload),
                update_hover_materials.after(InteractionSystems::Selection),
                update_visual_layer_visibility,
                apply_visualization_mode.after(update_visual_layer_visibility),
                update_contact_review_pose,
                apply_contact_review
                    .after(update_contact_review_pose)
                    .after(update_hover_materials)
                    .after(apply_visualization_mode),
                update_topology_highlights.after(InteractionSystems::Selection),
                update_contact_candidate_highlights
                    .after(update_contact_review_pose)
                    .after(apply_contact_review),
                update_contour_surface,
                update_colorbar,
                spawn_boundary_visuals,
                spawn_boundary_load_preview,
            ),
        );
    }
}
