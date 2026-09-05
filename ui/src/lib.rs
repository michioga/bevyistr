mod assembly;
mod assembly_clearance;
mod assembly_ui;
mod bc_loads_ui;
mod boundary_editor;
mod contact_ui;
mod layout;
mod load_direction;
mod material_editor;
mod material_catalog;
mod material_assignment;
mod material_library;
mod materials_ui;
mod measurement;
mod mpc_ui;
mod project_io;
mod results_ui;
mod selection_ui;
pub mod slider;
mod solve_ui;
mod solver_editor;
mod solver_process;
mod solver_runner;

use bevy::prelude::*;
use interaction::InteractionSystems;

use assembly::{
    AssemblyEditorState, assembly_viewport_hover_system, assembly_viewport_input_system,
    spawn_assembly_viewport_visuals, sync_assembly_overlay_camera, update_assembly_gizmo_visuals,
    update_assembly_part_overlays,
};
use assembly_clearance::{
    AssemblyClearanceGizmos, AssemblyClearanceState, assembly_clearance_button_system,
    assembly_clearance_review_button_system, draw_assembly_clearance_preview,
    sync_assembly_clearance_review, update_assembly_clearance_controls,
    update_assembly_clearance_text,
};
use assembly_ui::{
    assembly_part_button_system, assembly_tool_button_system, assembly_transform_button_system,
    rebuild_assembly_parts, update_assembly_nudge_visibility, update_assembly_status_text,
};

use bc_loads_ui::{
    ActiveLoadEditor, SelectedDloadKind, SelectedLoadDirection, apply_dload_button_system,
    apply_load_button_system, clear_all_bc_loads_button_system, constraint_preset_button_system,
    dload_kind_button_system, load_direction_button_system, rebuild_boundary_loads_list,
    sync_load_measurement_box, toggle_constraints_button_system, toggle_loads_button_system,
    update_apply_dload_label, update_apply_load_label, update_boundary_load_preview,
    update_constraint_button_labels,
};

use boundary_editor::{
    BoundaryLoadEditorState, QuickLoadControlState, apply_constraint_button_system,
    constraint_dof_toggle_system, engineering_numeric_input_system, rotation_center_button_system,
    rotational_input_mode_button_system, sync_quick_load_controls, update_apply_constraint_label,
    update_dload_exact_field_visibility, update_rotation_center_status,
};
use contact_ui::{
    ContactDefinitionSettings, accept_contact_button_system, capture_contact_side_button_system,
    contact_behavior_button_system, contact_candidate_action_button_system,
    contact_ghost_toggle_button_system, contact_pair_kind_button_system,
    contact_parameter_button_system, contact_penalty_toggle_button_system,
    create_contact_button_system, create_surface_button_system, defined_contact_button_system,
    detect_contacts_button_system, finalize_contact_button_system,
    rebuild_contact_definitions_list, sync_contact_measurement_box, sync_contact_search_params,
    sync_defined_contact_preview, update_contact_candidate_text, update_contact_draft_status,
    update_contact_parameter_controls, update_contact_review_controls,
    update_contact_review_settings,
};
use layout::{
    SidebarPage, UndoInProgress, UndoStack, delete_setup_entry_button_system, handle_panel_wheel,
    handle_scrollable_list_wheel, push_undo_before_setup_change, render_mode_button_system,
    sidebar_page_button_system, spawn_ui, undo_redo_system, update_sidebar_page_visibility,
    update_ui_pointer_state,
};
use load_direction::{
    LoadDirectionPickerState, load_direction_picker_button_system,
    load_direction_picker_hover_system, load_direction_picker_input_system,
    spawn_load_direction_gizmo, update_load_direction_gizmo_visuals,
};
use material_editor::{MaterialEditorState, material_numeric_input_system};
use material_library::{MaterialLibraryState, material_library_system};
use material_assignment::{MaterialViewportHover, material_assignment_tool, material_assignment_hover, material_assignment_click, draw_material_target};
use materials_ui::{
    SelectedEgrp, SelectedMaterialForSection, SelectedSectionType, add_section_button_system,
    egrp_select_button_system, material_color_button_system, material_select_button_system,
    rebuild_materials_sections_list, rebuild_section_def_panel, section_type_button_system,
    update_material_workflow,
};
use measurement::{
    MeasurementBoxState, measurement_box_input_system, spawn_measurement_box,
    update_measurement_box_visuals, update_ui_keyboard_state,
};
use mpc_ui::{
    MpcEquationEditorState, MpcPairDraftState, accept_rigid_spider_button_system,
    capture_mpc_pair_node_button_system, clear_mpc_pair_button_system,
    create_mpc_pair_button_system, defined_mpc_action_button_system,
    detect_rigid_spiders_button_system, mpc_pair_dof_button_system,
    rigid_spider_action_button_system, sync_defined_mpc_preview, sync_mpc_pair_draft_preview,
    sync_rigid_spider_review, sync_rigid_spider_search_params, update_defined_mpc_text,
    update_mpc_pair_draft_text, update_rigid_spider_candidate_text,
};
use project_io::{
    CameraFitRequest, add_mesh_button_system, apply_pending_cnt_system, camera_refit_on_reload,
    export_button_system, mesh_load_system, open_mesh_button_system, open_project_button_system,
    open_setup_button_system, update_mesh_stats_text,
};
use results_ui::{
    PlaybackState, apply_slider_to_results, open_result_button_system, playback_advance_system,
    playback_button_system, step_keyboard_navigation, update_result_stats_text,
};
use selection_ui::{
    SelectionGuideState, SurfaceSelectionSettings, make_element_group_button_system,
    make_node_group_button_system, rebuild_sets_list, selection_guide_toggle_system,
    selection_level_button_system, set_button_system, surface_selection_mode_button_system,
    update_hover_preview_group, update_selection_context, update_selection_info_text,
    update_selection_operation_hint, update_selection_stats_text, update_surface_selection_hint,
};
use solve_ui::{
    analysis_type_button_system, solver_method_button_system, update_analysis_setup_stats_text,
};
use solver_editor::{SolverEditorState, solver_numeric_input_system};
use solver_runner::{
    FrontistrRunState, mpi_rank_adjust_button_system, poll_frontistr_process_system,
    run_frontistr_button_system, select_frontistr_executable_system,
    solver_launch_mode_button_system, stop_frontistr_button_system, update_frontistr_run_ui_system,
    update_mpi_rank_controls_system,
};

pub use slider::{SliderConfig, SliderId, SliderState, SliderThumb, SliderTrack, spawn_slider};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<AssemblyClearanceGizmos>();
        {
            let mut configs = app.world_mut().resource_mut::<GizmoConfigStore>();
            let (config, _) = configs.config_mut::<AssemblyClearanceGizmos>();
            config.line.width = 3.0;
            config.depth_bias = -0.002;
        }
        app.init_resource::<fem_core::MeshLoadRequest>();
        app.init_resource::<fem_core::MeshLoadStatus>();
        app.init_resource::<fem_core::FemModelVersion>();
        app.init_resource::<fem_core::PendingCntLoad>();
        app.init_resource::<fem_core::UiPointerState>();
        app.init_resource::<fem_core::UiKeyboardState>();
        app.init_resource::<fem_core::ViewportTool>();
        app.init_resource::<fem_core::ContactCandidateState>();
        app.init_resource::<fem_core::RigidSpiderCandidateState>();
        app.init_resource::<fem_core::FemResultSet>();
        app.init_resource::<fem_core::AnalysisSetup>();
        app.init_resource::<fem_core::HoverPreviewTargets>();
        app.init_resource::<visualization::VisualizationSettings>();
        app.init_resource::<visualization::ContactReviewSettings>();
        app.init_resource::<visualization::DefinedContactPreview>();
        app.init_resource::<visualization::ContactDraftPreview>();
        app.init_resource::<visualization::RigidSpiderReviewSettings>();
        app.init_resource::<visualization::DefinedMpcPreview>();
        app.init_resource::<visualization::MpcPairDraftPreview>();
        app.init_resource::<visualization::BoundaryVisualSettings>();
        app.init_resource::<visualization::BoundaryLoadPreview>();
        app.init_resource::<SidebarPage>();
        app.init_resource::<AssemblyEditorState>();
        app.init_resource::<AssemblyClearanceState>();
        app.init_resource::<BoundaryLoadEditorState>();
        app.init_resource::<QuickLoadControlState>();
        app.init_resource::<MeasurementBoxState>();
        app.init_resource::<CameraFitRequest>();
        app.init_resource::<SelectionGuideState>();
        app.init_resource::<SelectedLoadDirection>();
        app.init_resource::<LoadDirectionPickerState>();
        app.init_resource::<ActiveLoadEditor>();
        app.init_resource::<SelectedSectionType>();
        app.init_resource::<SelectedEgrp>();
        app.init_resource::<SelectedMaterialForSection>();
        app.init_resource::<MaterialEditorState>();
        app.init_resource::<MaterialLibraryState>();
        app.init_resource::<MaterialViewportHover>();
        app.init_resource::<visualization::MaterialColorMode>();
        app.init_resource::<SurfaceSelectionSettings>();
        app.init_resource::<ContactDefinitionSettings>();
        app.init_resource::<MpcEquationEditorState>();
        app.init_resource::<MpcPairDraftState>();
        app.init_resource::<SolverEditorState>();
        app.init_resource::<FrontistrRunState>();
        app.init_resource::<SelectedDloadKind>();
        app.init_resource::<PlaybackState>();
        app.init_resource::<UndoStack>();
        app.init_resource::<UndoInProgress>();
        app.add_systems(
            Startup,
            (
                spawn_ui,
                spawn_assembly_viewport_visuals,
                spawn_load_direction_gizmo,
                spawn_measurement_box,
            ),
        );

        // Shared text-input capture and exact viewport value submission.
        app.add_systems(
            Update,
            (
                update_ui_keyboard_state
                    .before(measurement_box_input_system)
                    .before(engineering_numeric_input_system)
                    .before(solver_numeric_input_system)
                    .before(material_numeric_input_system)
                    .before(selection::selection_filter_shortcut_system)
                    .before(selection::clear_selection_shortcut_system),
                measurement_box_input_system,
                engineering_numeric_input_system.after(update_ui_keyboard_state),
                solver_numeric_input_system.after(update_ui_keyboard_state),
            )
                .in_set(InteractionSystems::UiInput),
        );

        app.add_systems(
            Update,
            material_numeric_input_system
                .after(update_ui_keyboard_state)
                .after(sidebar_page_button_system)
                .after(material_library_system)
                .after(material_select_button_system)
                .after(delete_setup_entry_button_system)
                .after(apply_pending_cnt_system)
                .before(add_section_button_system)
                .in_set(InteractionSystems::UiInput),
        );

        // Group 1: pointer, navigation, mesh loading (≤16 systems)
        app.add_systems(
            Update,
            (
                update_ui_pointer_state.in_set(InteractionSystems::UiInput),
                handle_scrollable_list_wheel.in_set(InteractionSystems::UiInput),
                handle_panel_wheel.in_set(InteractionSystems::UiInput),
                sidebar_page_button_system,
                update_sidebar_page_visibility.after(sidebar_page_button_system),
                open_project_button_system,
                open_mesh_button_system,
                add_mesh_button_system,
                mesh_load_system,
                camera_refit_on_reload.after(mesh_load_system),
                rebuild_sets_list.after(mesh_load_system),
                set_button_system,
                make_node_group_button_system,
                make_element_group_button_system,
                selection_level_button_system.in_set(InteractionSystems::UiInput),
                render_mode_button_system,
            ),
        );
        app.add_systems(
            Update,
            update_selection_context
                .after(sidebar_page_button_system)
                .after(selection::selection_filter_shortcut_system)
                .before(selection_level_button_system)
                .in_set(InteractionSystems::UiInput),
        );

        // Group 2: contact, results, playback (≤12 systems)
        app.add_systems(
            Update,
            (
                create_surface_button_system,
                create_contact_button_system,
                detect_contacts_button_system.after(sync_contact_search_params),
                accept_contact_button_system
                    .after(contact_behavior_button_system)
                    .after(contact_penalty_toggle_button_system)
                    .after(slider::update_sliders),
                contact_candidate_action_button_system,
                contact_ghost_toggle_button_system,
                update_contact_review_controls
                    .after(detect_contacts_button_system)
                    .after(contact_candidate_action_button_system)
                    .after(accept_contact_button_system),
                open_result_button_system,
                playback_button_system,
                playback_advance_system,
                apply_pending_cnt_system.after(mesh_load_system),
            ),
        );
        app.add_systems(
            Update,
            (
                capture_mpc_pair_node_button_system,
                mpc_pair_dof_button_system,
                create_mpc_pair_button_system
                    .after(capture_mpc_pair_node_button_system)
                    .after(mpc_pair_dof_button_system),
                clear_mpc_pair_button_system.after(capture_mpc_pair_node_button_system),
                sync_mpc_pair_draft_preview
                    .after(sidebar_page_button_system)
                    .after(capture_mpc_pair_node_button_system)
                    .after(create_mpc_pair_button_system)
                    .after(clear_mpc_pair_button_system)
                    .after(detect_contacts_button_system)
                    .after(detect_rigid_spiders_button_system),
                update_mpc_pair_draft_text
                    .after(capture_mpc_pair_node_button_system)
                    .after(create_mpc_pair_button_system)
                    .after(clear_mpc_pair_button_system)
                    .after(mpc_pair_dof_button_system),
            ),
        );
        app.add_systems(
            Update,
            (
                detect_rigid_spiders_button_system.after(sync_rigid_spider_search_params),
                rigid_spider_action_button_system,
                accept_rigid_spider_button_system,
                update_rigid_spider_candidate_text
                    .after(detect_rigid_spiders_button_system)
                    .after(rigid_spider_action_button_system)
                    .after(accept_rigid_spider_button_system),
                sync_rigid_spider_review
                    .after(sidebar_page_button_system)
                    .after(detect_rigid_spiders_button_system)
                    .after(rigid_spider_action_button_system)
                    .after(accept_rigid_spider_button_system)
                    .after(defined_mpc_action_button_system),
            ),
        );
        app.add_systems(
            Update,
            (
                defined_mpc_action_button_system.after(accept_rigid_spider_button_system),
                sync_defined_mpc_preview
                    .after(sidebar_page_button_system)
                    .after(mesh_load_system)
                    .after(apply_pending_cnt_system)
                    .after(defined_mpc_action_button_system)
                    .after(sync_mpc_pair_draft_preview)
                    .after(detect_contacts_button_system)
                    .after(detect_rigid_spiders_button_system),
                update_defined_mpc_text
                    .after(sync_defined_mpc_preview)
                    .after(accept_rigid_spider_button_system),
            ),
        );
        app.add_systems(
            Update,
            (
                contact_pair_kind_button_system,
                contact_behavior_button_system.after(contact_pair_kind_button_system),
                contact_penalty_toggle_button_system.after(contact_behavior_button_system),
                contact_parameter_button_system.after(contact_penalty_toggle_button_system),
                sync_contact_search_params.after(slider::update_sliders),
                sync_rigid_spider_search_params.after(slider::update_sliders),
                update_contact_parameter_controls
                    .after(contact_behavior_button_system)
                    .after(contact_penalty_toggle_button_system),
                sync_contact_measurement_box
                    .after(contact_behavior_button_system)
                    .after(contact_penalty_toggle_button_system)
                    .after(contact_parameter_button_system)
                    .after(defined_mpc_action_button_system)
                    .after(slider::update_sliders),
                capture_contact_side_button_system.after(contact_pair_kind_button_system),
                finalize_contact_button_system
                    .after(capture_contact_side_button_system)
                    .after(slider::update_sliders),
                update_contact_draft_status
                    .after(contact_pair_kind_button_system)
                    .after(contact_behavior_button_system)
                    .after(capture_contact_side_button_system)
                    .after(finalize_contact_button_system),
            ),
        );
        app.add_systems(
            Update,
            (
                rebuild_contact_definitions_list
                    .after(mesh_load_system)
                    .after(apply_pending_cnt_system),
                defined_contact_button_system
                    .after(rebuild_contact_definitions_list)
                    .after(defined_mpc_action_button_system),
                sync_defined_contact_preview
                    .after(sidebar_page_button_system)
                    .after(rebuild_contact_definitions_list)
                    .after(defined_contact_button_system)
                    .after(sync_defined_mpc_preview)
                    .after(sync_mpc_pair_draft_preview)
                    .after(detect_contacts_button_system)
                    .after(contact_candidate_action_button_system)
                    .after(accept_contact_button_system)
                    .after(capture_contact_side_button_system)
                    .after(finalize_contact_button_system),
            ),
        );

        // Group 3: analysis setup — BCs, loads, materials
        app.add_systems(
            Update,
            (
                export_button_system,
                open_setup_button_system,
                constraint_preset_button_system,
                load_direction_button_system,
                load_direction_picker_button_system.in_set(InteractionSystems::UiInput),
                apply_load_button_system,
                dload_kind_button_system,
                apply_dload_button_system,
                sync_load_measurement_box
                    .after(load_direction_button_system)
                    .after(dload_kind_button_system)
                    .after(sync_quick_load_controls)
                    .after(slider::update_sliders),
                update_boundary_load_preview
                    .after(load_direction_button_system)
                    .after(dload_kind_button_system)
                    .after(sync_quick_load_controls)
                    .after(slider::update_sliders),
                material_library_system.after(sidebar_page_button_system).after(egrp_select_button_system),
                material_select_button_system.after(material_library_system),
                delete_setup_entry_button_system,
                clear_all_bc_loads_button_system,
                section_type_button_system,
                egrp_select_button_system,
                add_section_button_system.after(egrp_select_button_system),
                analysis_type_button_system,
                solver_method_button_system,
            ),
        );
        app.add_systems(
            Update,
            (
                select_frontistr_executable_system,
                solver_launch_mode_button_system,
                mpi_rank_adjust_button_system.after(solver_launch_mode_button_system),
                run_frontistr_button_system
                    .after(solver_launch_mode_button_system)
                    .after(mpi_rank_adjust_button_system)
                    .after(select_frontistr_executable_system)
                    .after(export_button_system)
                    .after(solver_numeric_input_system)
                    .after(analysis_type_button_system)
                    .after(solver_method_button_system),
                stop_frontistr_button_system,
                poll_frontistr_process_system
                    .after(run_frontistr_button_system)
                    .after(stop_frontistr_button_system),
                update_frontistr_run_ui_system.after(poll_frontistr_process_system),
                update_mpi_rank_controls_system
                    .after(solver_launch_mode_button_system)
                    .after(mpi_rank_adjust_button_system),
            ),
        );

        // Group 4: UI rebuild + toggles (≤10 systems)
        app.add_systems(
            Update,
            (
                rebuild_section_def_panel
                    .after(material_numeric_input_system)
                    .before(add_section_button_system),
                update_material_workflow.after(rebuild_section_def_panel).after(egrp_select_button_system),
                rebuild_boundary_loads_list,
                rebuild_materials_sections_list.after(material_numeric_input_system),
                toggle_constraints_button_system,
                toggle_loads_button_system,
                surface_selection_mode_button_system
                    .after(update_selection_context)
                    .after(selection_level_button_system)
                    .in_set(InteractionSystems::UiInput),
                selection_guide_toggle_system.in_set(InteractionSystems::UiInput),
                update_selection_operation_hint.in_set(InteractionSystems::UiInput),
                update_hover_preview_group
                    .after(slider::update_sliders)
                    .in_set(InteractionSystems::Preview),
            ),
        );

        // Group 5: undo/redo, surface selection, sliders, text updates (≤16 systems)
        app.add_systems(
            Update,
            (
                push_undo_before_setup_change
                    .after(defined_mpc_action_button_system)
                    .after(create_mpc_pair_button_system)
                    .after(solver_numeric_input_system)
                    .after(material_numeric_input_system)
                    .after(add_section_button_system)
                    .after(measurement_box_input_system),
                undo_redo_system
                    .after(push_undo_before_setup_change)
                    .after(update_ui_keyboard_state),
                update_selection_info_text,
                update_surface_selection_hint.after(update_selection_context),
                update_constraint_button_labels,
                update_apply_load_label,
                update_apply_dload_label,
                step_keyboard_navigation.after(update_ui_keyboard_state),
                slider::update_sliders.after(step_keyboard_navigation),
                update_contact_review_settings
                    .after(sidebar_page_button_system)
                    .after(contact_candidate_action_button_system)
                    .after(accept_contact_button_system)
                    .after(slider::update_sliders),
                apply_slider_to_results.after(slider::update_sliders),
                update_mesh_stats_text,
                update_selection_stats_text,
                update_contact_candidate_text,
                update_result_stats_text,
                update_analysis_setup_stats_text,
            ),
        );

        app.add_systems(
            Update,
            material_color_button_system.in_set(InteractionSystems::UiInput),
        );
        app.add_systems(Update, material_assignment_tool.after(sidebar_page_button_system)
            .after(assembly_tool_button_system).in_set(InteractionSystems::UiInput));
        app.add_systems(Update, material_assignment_hover.in_set(InteractionSystems::Picking));
        app.add_systems(Update, material_assignment_click.in_set(InteractionSystems::Selection));
        app.add_systems(Update, draw_material_target.after(InteractionSystems::Selection));

        // Group 6: assembly part selection and pose editing.
        app.add_systems(
            Update,
            (
                rebuild_assembly_parts.after(mesh_load_system),
                assembly_tool_button_system
                    .after(sidebar_page_button_system)
                    .in_set(InteractionSystems::UiInput),
                update_assembly_nudge_visibility
                    .after(assembly_tool_button_system)
                    .in_set(InteractionSystems::UiInput),
                assembly_part_button_system
                    .after(rebuild_assembly_parts)
                    .in_set(InteractionSystems::UiInput),
                assembly_transform_button_system
                    .after(assembly_tool_button_system)
                    .after(assembly_part_button_system)
                    .after(sidebar_page_button_system)
                    .after(slider::update_sliders)
                    .in_set(InteractionSystems::UiInput),
                assembly_clearance_button_system
                    .after(assembly_transform_button_system)
                    .after(update_contact_review_settings)
                    .in_set(InteractionSystems::UiInput),
                assembly_clearance_review_button_system
                    .after(assembly_clearance_button_system)
                    .in_set(InteractionSystems::UiInput),
                update_assembly_status_text.after(assembly_transform_button_system),
            ),
        );

        // Invalidate after every geometry/part edit, including drag release and
        // exact input, before presenting any clearance measurements this frame.
        app.add_systems(
            Update,
            sync_assembly_clearance_review
                .after(InteractionSystems::Selection)
                .after(mesh_load_system)
                .after(update_contact_review_settings),
        );
        app.add_systems(
            Update,
            (
                update_assembly_clearance_text,
                update_assembly_clearance_controls,
                draw_assembly_clearance_preview,
            )
                .after(sync_assembly_clearance_review),
        );

        app.add_systems(
            Update,
            (
                assembly_viewport_hover_system,
                load_direction_picker_hover_system,
            )
                .in_set(InteractionSystems::Picking),
        );
        app.add_systems(
            Update,
            (
                constraint_dof_toggle_system.in_set(InteractionSystems::UiInput),
                rotational_input_mode_button_system.in_set(InteractionSystems::UiInput),
                rotation_center_button_system.in_set(InteractionSystems::UiInput),
                apply_constraint_button_system.after(constraint_preset_button_system),
                sync_quick_load_controls
                    .after(engineering_numeric_input_system)
                    .after(load_direction_button_system)
                    .after(dload_kind_button_system)
                    .after(slider::update_sliders),
                update_apply_constraint_label
                    .after(constraint_preset_button_system)
                    .after(constraint_dof_toggle_system)
                    .after(rotational_input_mode_button_system)
                    .after(rotation_center_button_system),
                update_rotation_center_status
                    .after(rotational_input_mode_button_system)
                    .after(rotation_center_button_system),
                update_dload_exact_field_visibility.after(dload_kind_button_system),
            ),
        );
        app.add_systems(
            Update,
            (
                assembly_viewport_input_system,
                load_direction_picker_input_system,
            )
                .in_set(InteractionSystems::Selection),
        );
        app.add_systems(
            Update,
            (
                update_assembly_part_overlays,
                update_assembly_gizmo_visuals,
                update_load_direction_gizmo_visuals,
                sync_assembly_overlay_camera,
                update_measurement_box_visuals
                    .after(sync_load_measurement_box)
                    .after(sync_contact_measurement_box),
            )
                .after(assembly_viewport_input_system)
                .after(load_direction_picker_input_system),
        );
    }
}
