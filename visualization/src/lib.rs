mod boundary_viz;
mod colorbar;
mod contour;
mod demo_mesh;
mod material_colors;
pub use material_colors::{
    INVALID_MATERIAL_COLOR, MaterialColorMode, UNASSIGNED_MATERIAL_COLOR, material_identity_color,
};

use bevy::prelude::*;
use interaction::InteractionSystems;

use boundary_viz::{spawn_boundary_load_preview, spawn_boundary_visuals};
use colorbar::{spawn_colorbar, update_colorbar};
use contour::{ContourSurface, apply_contour_visibility, update_contour_surface};
use demo_mesh::{
    apply_contact_review, apply_visualization_mode, respawn_elements_on_setup_change,
    respawn_visuals_on_reload, restore_selection_on_new_visuals,
    spawn_contact_candidate_highlights, spawn_demo_mesh, spawn_rigid_spider_highlights,
    spawn_topology_highlights, update_contact_candidate_highlights, update_contact_review_pose,
    update_hover_materials, update_rigid_spider_highlights, update_topology_highlights,
    update_visual_layer_visibility,
};

pub use boundary_viz::{
    BoundaryLoadPreview, BoundaryLoadPreviewArrow, BoundaryLoadPreviewKind,
    BoundaryLoadPreviewMoment, BoundaryLoadPreviewVisual, BoundaryVisual, BoundaryVisualSettings,
};
pub use colorbar::ColorbagRoot;
pub use demo_mesh::{
    ContactDraftPreview, ContactDraftSlave, ContactDraftSurface, ContactReviewSettings,
    ContourSettings, DefinedContactPreview, DefinedMpcPreview, FemMeshVisual, FemPartVisual,
    FlatMaterial, MpcPairDraftPreview, RigidSpiderReviewSettings, TransparentMaterial,
    VisualizationMode, VisualizationSettings, build_part_edge_mesh, build_part_surface_mesh,
};

pub struct VisualizationPlugin;

impl Plugin for VisualizationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VisualizationSettings>();
        app.init_resource::<MaterialColorMode>();
        app.init_resource::<ContourSurface>();
        app.init_resource::<ContactReviewSettings>();
        app.init_resource::<DefinedContactPreview>();
        app.init_resource::<ContactDraftPreview>();
        app.init_resource::<RigidSpiderReviewSettings>();
        app.init_resource::<DefinedMpcPreview>();
        app.init_resource::<MpcPairDraftPreview>();
        app.init_resource::<demo_mesh::ContactReviewPose>();
        app.init_resource::<fem_core::FemModelVersion>();
        app.init_resource::<fem_core::ContactCandidateState>();
        app.init_resource::<fem_core::RigidSpiderCandidateState>();
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
                spawn_rigid_spider_highlights,
                spawn_colorbar,
            ),
        );

        app.add_systems(
            Update,
            (
                respawn_visuals_on_reload,
                respawn_elements_on_setup_change.after(respawn_visuals_on_reload),
                restore_selection_on_new_visuals
                    .after(respawn_elements_on_setup_change)
                    .after(InteractionSystems::Selection),
                update_hover_materials
                    .after(InteractionSystems::Selection)
                    .after(restore_selection_on_new_visuals)
                    .after(apply_visualization_mode),
                update_visual_layer_visibility.after(respawn_elements_on_setup_change),
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
                update_rigid_spider_highlights.after(update_contact_candidate_highlights),
                update_contour_surface
                    .after(apply_contact_review)
                    .after(respawn_elements_on_setup_change),
                apply_contour_visibility.after(update_contour_surface),
                update_colorbar,
                spawn_boundary_visuals,
                spawn_boundary_load_preview,
            ),
        );
    }
}
