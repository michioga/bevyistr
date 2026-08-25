mod layout;
pub mod slider;

use bevy::prelude::*;
use interaction::InteractionSystems;

use layout::{
    accept_contact_button_system, add_section_button_system, analysis_type_button_system,
    apply_dload_button_system, apply_load_button_system, apply_pending_cnt_system,
    apply_slider_to_results,
    camera_refit_on_reload, clear_all_bc_loads_button_system, constraint_preset_button_system,
    create_contact_button_system, create_surface_button_system,
    delete_setup_entry_button_system, detect_contacts_button_system,
    dload_kind_button_system, egrp_select_button_system, export_button_system,
    handle_panel_wheel, handle_scrollable_list_wheel, import_mesh_button_system,
    load_direction_button_system, make_element_group_button_system,
    make_node_group_button_system, material_preset_button_system,
    material_select_button_system, mesh_load_system, open_mesh_button_system,
    open_project_button_system, open_result_button_system, open_setup_button_system,
    surface_selection_mode_button_system,
    playback_advance_system, playback_button_system, push_undo_before_setup_change,
    rebuild_boundary_loads_list, rebuild_materials_sections_list, rebuild_section_def_panel,
    rebuild_sets_list, render_mode_button_system, section_type_button_system,
    selection_level_button_system, set_button_system, sidebar_page_button_system,
    solver_method_button_system,
    spawn_ui, step_keyboard_navigation, toggle_constraints_button_system,
    toggle_loads_button_system, undo_redo_system,
    update_analysis_setup_stats_text, update_apply_dload_label, update_apply_load_label,
    update_constraint_button_labels, update_contact_candidate_text, update_mesh_stats_text,
    update_hover_preview_group,
    update_surface_selection_hint,
    update_parts_list_text, update_result_stats_text, update_selection_info_text,
    update_selection_operation_hint, update_selection_stats_text,
    update_sidebar_page_visibility, update_ui_pointer_state, selection_guide_toggle_system,
    SelectionGuideState, SurfaceSelectionSettings, PlaybackState, SelectedDloadKind,
    SelectedEgrp, SelectedLoadDirection, SelectedMaterialForSection, SelectedSectionType,
    SidebarPage, UndoInProgress, UndoStack,
};

pub use slider::{SliderConfig, SliderId, SliderState, SliderTrack, SliderThumb, spawn_slider};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<fem_core::MeshLoadRequest>();
        app.init_resource::<fem_core::MeshLoadStatus>();
        app.init_resource::<fem_core::FemModelVersion>();
        app.init_resource::<fem_core::PendingCntLoad>();
        app.init_resource::<fem_core::UiPointerState>();
        app.init_resource::<fem_core::ContactCandidateState>();
        app.init_resource::<fem_core::FemResultSet>();
        app.init_resource::<fem_core::AnalysisSetup>();
        app.init_resource::<fem_core::HoverPreviewTargets>();
        app.init_resource::<visualization::VisualizationSettings>();
        app.init_resource::<visualization::BoundaryVisualSettings>();
        app.init_resource::<SidebarPage>();
        app.init_resource::<SelectionGuideState>();
        app.init_resource::<SelectedLoadDirection>();
        app.init_resource::<SelectedSectionType>();
        app.init_resource::<SelectedEgrp>();
        app.init_resource::<SelectedMaterialForSection>();
        app.init_resource::<SurfaceSelectionSettings>();
        app.init_resource::<SelectedDloadKind>();
        app.init_resource::<PlaybackState>();
        app.init_resource::<UndoStack>();
        app.init_resource::<UndoInProgress>();
        app.add_systems(Startup, spawn_ui);

        // Group 1: pointer, navigation, mesh loading (≤16 systems)
        app.add_systems(Update, (
            update_ui_pointer_state.in_set(InteractionSystems::UiInput),
            handle_scrollable_list_wheel.in_set(InteractionSystems::UiInput),
            handle_panel_wheel.in_set(InteractionSystems::UiInput),
            sidebar_page_button_system,
            update_sidebar_page_visibility.after(sidebar_page_button_system),
            open_project_button_system,
            open_mesh_button_system,
            import_mesh_button_system,
            mesh_load_system,
            camera_refit_on_reload.after(mesh_load_system),
            rebuild_sets_list.after(mesh_load_system),
            set_button_system,
            make_node_group_button_system,
            make_element_group_button_system,
            selection_level_button_system.in_set(InteractionSystems::UiInput),
            render_mode_button_system,
        ));

        // Group 2: contact, results, playback (≤10 systems)
        app.add_systems(Update, (
            create_surface_button_system,
            create_contact_button_system,
            detect_contacts_button_system,
            accept_contact_button_system,
            open_result_button_system,
            playback_button_system,
            playback_advance_system,
            apply_pending_cnt_system.after(mesh_load_system),
        ));

        // Group 3: analysis setup — BCs, loads, materials (≤16 systems)
        app.add_systems(Update, (
            export_button_system,
            open_setup_button_system,
            constraint_preset_button_system,
            load_direction_button_system,
            apply_load_button_system,
            dload_kind_button_system,
            apply_dload_button_system,
            material_preset_button_system,
            material_select_button_system,
            delete_setup_entry_button_system,
            clear_all_bc_loads_button_system,
            section_type_button_system,
            egrp_select_button_system,
            add_section_button_system,
            analysis_type_button_system,
            solver_method_button_system,
        ));

        // Group 4: UI rebuild + toggles (≤10 systems)
        app.add_systems(Update, (
            rebuild_section_def_panel,
            rebuild_boundary_loads_list,
            rebuild_materials_sections_list,
            toggle_constraints_button_system,
            toggle_loads_button_system,
            surface_selection_mode_button_system
                .after(selection_level_button_system)
                .in_set(InteractionSystems::UiInput),
            selection_guide_toggle_system.in_set(InteractionSystems::UiInput),
            update_selection_operation_hint.in_set(InteractionSystems::UiInput),
            update_hover_preview_group
                .after(slider::update_sliders)
                .in_set(InteractionSystems::Preview),
        ));

        // Group 5: undo/redo, surface selection, sliders, text updates (≤16 systems)
        app.add_systems(Update, (
            push_undo_before_setup_change,
            undo_redo_system.after(push_undo_before_setup_change),
            update_selection_info_text,
            update_surface_selection_hint,
            update_constraint_button_labels,
            update_apply_load_label,
            update_apply_dload_label,
            step_keyboard_navigation,
            slider::update_sliders.after(step_keyboard_navigation),
            apply_slider_to_results.after(slider::update_sliders),
            update_mesh_stats_text,
            update_parts_list_text,
            update_selection_stats_text,
            update_contact_candidate_text,
            update_result_stats_text,
            update_analysis_setup_stats_text,
        ));
    }
}
