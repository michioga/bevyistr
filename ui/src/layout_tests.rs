use std::path::PathBuf;

use super::{
    SidebarPage, SidebarPageContent, page_supports_part_position, page_supports_tool,
    sidebar_page_display,
};
use crate::bc_loads_ui::{selected_nodes_by_mesh, signed_preview_direction};
use crate::contact_ui::{
    ContactDefinitionSettings, ContactPairKind, create_contact_from_candidate,
    create_contact_from_draft, sync_contact_measurement_box,
};
use crate::measurement::{MeasurementBoxState, MeasurementTarget};
use crate::mpc_ui::{next_mpc_pair_name, pair_mpc_equations, remove_mpc_equation_or_group};
use crate::project_io::{CameraFitRequest, apply_mesh, merge_mesh_contact_pairs};
use crate::selection_ui::{
    SELECTION_GUIDE_TEXT, SelectionGuideState, SurfaceSelectionMode, SurfaceSelectionSettings,
    selection_context_for_page, selection_operation_hint, supports_surface_growth,
    surface_selection_hint, update_hover_preview_group,
};
use bevy::prelude::{App, Display, Update, Vec3};
use fem_core::{
    AnalysisSetup, ContactCandidate, ElementId, ElementType, FemElement, FemEntityId, FemEntityRef,
    FemMesh, FemModel, FemModelVersion, FemNode, HoverPreviewTargets, MeshLoadStatus, MpcEquation,
    NodeId, SelectionHit, SelectionLevel, ViewportTool,
};
use interaction::HoverResult;
use selection::{SelectionOperation, SelectionState};
use visualization::{ContactDraftPreview, ContactDraftSlave, ContactDraftSurface};

#[test]
fn analysis_shell_is_limited_to_analysis_pages() {
    let pages = SidebarPageContent::analysis();

    assert!(!pages.contains(SidebarPage::Model));
    assert!(!pages.contains(SidebarPage::Contact));
    assert!(pages.contains(SidebarPage::Loads));
    assert!(pages.contains(SidebarPage::Materials));
    assert!(pages.contains(SidebarPage::Solve));
    assert!(!pages.contains(SidebarPage::Results));
}

#[test]
fn removing_a_grouped_mpc_keeps_unrelated_equations_intact() {
    let mut equations = vec![
        MpcEquation::new("SPIDER_1_U1", 0.0, Vec::new()).with_group("SPIDER_1"),
        MpcEquation::new("SPIDER_1_U2", 0.0, Vec::new()).with_group("SPIDER_1"),
        MpcEquation::new("INDEPENDENT", 0.0, Vec::new()),
    ];

    assert_eq!(remove_mpc_equation_or_group(&mut equations, 0), 2);
    assert_eq!(equations.len(), 1);
    assert_eq!(equations[0].name, "INDEPENDENT");
    assert_eq!(remove_mpc_equation_or_group(&mut equations, 0), 1);
    assert!(equations.is_empty());
}

#[test]
fn pair_mpc_xyz_builds_three_grouped_equal_displacement_equations() {
    let equations = pair_mpc_equations("MPC_PAIR_1", (0, NodeId(10)), (1, NodeId(20)), 0).unwrap();

    assert_eq!(equations.len(), 3);
    for (index, equation) in equations.iter().enumerate() {
        let dof = index as u8 + 1;
        assert_eq!(equation.group.as_deref(), Some("MPC_PAIR_1"));
        assert!(equation.is_valid());
        assert_eq!(equation.terms.len(), 2);
        assert_eq!(equation.terms[0].mesh_index, 0);
        assert_eq!(equation.terms[0].node, NodeId(10));
        assert_eq!(equation.terms[0].dof, dof);
        assert_eq!(equation.terms[0].coefficient, 1.0);
        assert_eq!(equation.terms[1].mesh_index, 1);
        assert_eq!(equation.terms[1].node, NodeId(20));
        assert_eq!(equation.terms[1].dof, dof);
        assert_eq!(equation.terms[1].coefficient, -1.0);
    }
}

#[test]
fn pair_mpc_single_dof_and_name_generation_are_collision_safe() {
    let equations = pair_mpc_equations("MPC_PAIR_2", (0, NodeId(1)), (0, NodeId(2)), 2).unwrap();
    assert_eq!(equations.len(), 1);
    assert_eq!(equations[0].name, "MPC_PAIR_2_UY");
    assert_eq!(equations[0].terms[0].dof, 2);

    let mut setup = AnalysisSetup::default();
    setup
        .mpc_equations
        .push(MpcEquation::new("MPC_PAIR_1_UX", 0.0, Vec::new()).with_group("MPC_PAIR_1"));
    setup
        .mpc_equations
        .push(MpcEquation::new("MPC_PAIR_2_UZ", 0.0, Vec::new()));
    assert_eq!(next_mpc_pair_name(&setup), "MPC_PAIR_3");
}

#[test]
fn part_position_controls_are_shared_by_model_and_contact() {
    let pages = SidebarPageContent::part_position();

    assert!(pages.contains(SidebarPage::Model));
    assert!(pages.contains(SidebarPage::Contact));
    assert!(!pages.contains(SidebarPage::Loads));
    assert!(page_supports_part_position(SidebarPage::Model));
    assert!(page_supports_part_position(SidebarPage::Contact));
    assert!(!page_supports_part_position(SidebarPage::Materials));
}

#[test]
fn viewport_tools_are_limited_to_their_workflow_pages() {
    assert!(page_supports_tool(
        SidebarPage::Contact,
        ViewportTool::Assembly
    ));
    assert!(!page_supports_tool(
        SidebarPage::Loads,
        ViewportTool::Assembly
    ));
    assert!(page_supports_tool(
        SidebarPage::Loads,
        ViewportTool::LoadDirection
    ));
    assert!(!page_supports_tool(
        SidebarPage::Model,
        ViewportTool::LoadDirection
    ));
}

#[test]
fn contact_measurement_sync_preserves_part_position_input() {
    let mut measurement = MeasurementBoxState::default();
    measurement.begin_assembly_translation(0, Vec3::X);

    let mut app = App::new();
    app.insert_resource(SidebarPage::Contact);
    app.insert_resource(ViewportTool::Assembly);
    app.init_resource::<ContactDefinitionSettings>();
    app.insert_resource(measurement);
    app.add_systems(Update, sync_contact_measurement_box);
    app.update();

    assert!(matches!(
        app.world().resource::<MeasurementBoxState>().target,
        Some(MeasurementTarget::AssemblyTranslation { .. })
    ));
}

#[test]
fn contact_measurement_sync_preserves_exact_mpc_input() {
    let mut measurement = MeasurementBoxState::default();
    measurement.begin_mpc_coefficient(2, 4, -0.5);

    let mut app = App::new();
    app.insert_resource(SidebarPage::Contact);
    app.insert_resource(ViewportTool::Selection);
    app.init_resource::<ContactDefinitionSettings>();
    app.insert_resource(measurement);
    app.add_systems(Update, sync_contact_measurement_box);
    app.update();

    assert!(matches!(
        app.world().resource::<MeasurementBoxState>().target,
        Some(MeasurementTarget::MpcCoefficient {
            equation: 2,
            term: 4
        })
    ));
}

#[test]
fn single_page_masks_do_not_leak_to_other_pages() {
    let all_pages = [
        SidebarPage::Model,
        SidebarPage::Contact,
        SidebarPage::Loads,
        SidebarPage::Materials,
        SidebarPage::Solve,
        SidebarPage::Results,
    ];

    for selected in all_pages {
        let content = SidebarPageContent::page(selected);
        for candidate in all_pages {
            assert_eq!(content.contains(candidate), selected == candidate);
        }
    }
}

#[test]
fn inactive_sidebar_pages_are_removed_from_layout() {
    let contact = SidebarPageContent::page(SidebarPage::Contact);

    assert_eq!(
        sidebar_page_display(contact, SidebarPage::Contact),
        Display::Flex
    );
    assert_eq!(
        sidebar_page_display(contact, SidebarPage::Model),
        Display::None
    );
}

#[test]
fn selection_targets_follow_the_active_workflow() {
    let model = selection_context_for_page(SidebarPage::Model);
    assert_eq!(model.levels.len(), 4);

    let contact = selection_context_for_page(SidebarPage::Contact);
    assert_eq!(
        contact.levels,
        &[
            SelectionLevel::Node,
            SelectionLevel::Face,
            SelectionLevel::Element,
        ]
    );
    assert_eq!(contact.preferred, SelectionLevel::Node);

    let loads = selection_context_for_page(SidebarPage::Loads);
    assert!(loads.levels.contains(&SelectionLevel::Node));
    assert!(!loads.levels.contains(&SelectionLevel::Edge));

    let materials = selection_context_for_page(SidebarPage::Materials);
    assert_eq!(materials.levels, &[SelectionLevel::Element]);

    assert!(
        selection_context_for_page(SidebarPage::Solve)
            .levels
            .is_empty()
    );
    assert!(
        selection_context_for_page(SidebarPage::Results)
            .levels
            .is_empty()
    );
}

#[test]
fn node_surface_draft_creates_groups_and_contact_only_when_finalized() {
    let mut model = FemModel::demo_hex8();
    let master_face = model.meshes[0].cached_boundary_faces()[0]
        .element_face_ref()
        .unwrap();
    let draft = ContactDraftPreview {
        master: Some(ContactDraftSurface {
            mesh_index: 0,
            surfaces: vec![master_face],
        }),
        slave: Some(ContactDraftSlave::Nodes {
            mesh_index: 0,
            nodes: vec![NodeId(0), NodeId(1)],
        }),
        active: true,
    };

    assert!(model.contacts.is_empty());
    assert!(model.meshes[0].node_sets.is_empty());
    assert!(model.meshes[0].surface_sets.is_empty());

    let index = create_contact_from_draft(
        &mut model,
        &draft,
        ContactPairKind::NodeSurface,
        fem_core::ContactType::FiniteSliding,
        0.15,
        Some(2.5e5),
    )
    .unwrap();

    assert_eq!(index, 0);
    assert_eq!(model.meshes[0].node_sets[0].nodes.len(), 2);
    assert_eq!(model.meshes[0].surface_sets[0].surfaces.len(), 1);
    assert_eq!(
        model.contacts[0].slave,
        fem_core::ContactSlaveRef::Nodes(fem_core::NodeSetRef::new(0, 0))
    );
    assert_eq!(
        model.contacts[0].contact_type,
        fem_core::ContactType::FiniteSliding
    );
    assert_eq!(model.contacts[0].friction_coefficient, 0.15);
    assert_eq!(model.contacts[0].penalty_factor, Some(2.5e5));
}

#[test]
fn sliding_contact_rejects_invalid_parameters_before_creating_groups() {
    let mut model = FemModel::demo_hex8();
    let master_face = model.meshes[0].cached_boundary_faces()[0]
        .element_face_ref()
        .unwrap();
    let draft = ContactDraftPreview {
        master: Some(ContactDraftSurface {
            mesh_index: 0,
            surfaces: vec![master_face],
        }),
        slave: Some(ContactDraftSlave::Nodes {
            mesh_index: 0,
            nodes: vec![NodeId(0)],
        }),
        active: true,
    };

    let friction_error = create_contact_from_draft(
        &mut model,
        &draft,
        ContactPairKind::NodeSurface,
        fem_core::ContactType::FiniteSliding,
        -0.1,
        None,
    )
    .unwrap_err();
    assert!(friction_error.contains("Friction coefficient"));

    let penalty_error = create_contact_from_draft(
        &mut model,
        &draft,
        ContactPairKind::NodeSurface,
        fem_core::ContactType::FiniteSliding,
        0.1,
        Some(0.0),
    )
    .unwrap_err();
    assert!(penalty_error.contains("penalty factor"));
    assert!(model.contacts.is_empty());
    assert!(model.meshes[0].node_sets.is_empty());
    assert!(model.meshes[0].surface_sets.is_empty());
}

#[test]
fn detected_contact_uses_selected_behavior_and_exact_parameters() {
    let mut model = FemModel::demo_hex8();
    model.add_mesh("Second", FemMesh::demo_hex8());
    let candidate = ContactCandidate {
        mesh_a: 0,
        mesh_b: 1,
        faces_a: vec![model.meshes[0].cached_boundary_faces()[0].id],
        faces_b: vec![model.meshes[1].cached_boundary_faces()[0].id],
        pair_count: 1,
        average_gap: 0.0,
    };

    let index = create_contact_from_candidate(
        &mut model,
        &candidate,
        fem_core::ContactType::FiniteSliding,
        0.27,
        Some(3.5e5),
    )
    .unwrap();

    assert_eq!(model.contacts[index].name, "CONTACT_1");
    assert_eq!(
        model.contacts[index].contact_type,
        fem_core::ContactType::FiniteSliding
    );
    assert_eq!(model.contacts[index].friction_coefficient, 0.27);
    assert_eq!(model.contacts[index].penalty_factor, Some(3.5e5));
    assert!(matches!(
        model.contacts[index].slave,
        fem_core::ContactSlaveRef::Surface(_)
    ));
}

#[test]
fn replacing_a_mesh_clears_setup_inside_the_load_transaction() {
    let mut model = FemModel::demo_hex8();
    let mut setup = AnalysisSetup::default();
    setup.add_constraint(0, vec![NodeId(0)], 1, 3, 0.0);
    let mut status = MeshLoadStatus::default();
    let mut version = FemModelVersion::default();
    let mut camera_fit = CameraFitRequest::default();

    apply_mesh(
        FemMesh::demo_hex8(),
        &PathBuf::from("replacement.msh"),
        false,
        &mut model,
        &mut status,
        &mut version,
        &mut camera_fit,
        &mut setup,
    );

    assert!(setup.is_empty());
    assert_eq!(version.value, 1);
    assert_eq!(camera_fit.revision, 1);
}

#[test]
fn importing_an_assembly_part_preserves_existing_setup() {
    let mut model = FemModel::demo_hex8();
    let mut setup = AnalysisSetup::default();
    setup.add_constraint(0, vec![NodeId(0)], 1, 3, 0.0);
    let mut status = MeshLoadStatus::default();
    let mut version = FemModelVersion::default();
    let mut camera_fit = CameraFitRequest::default();

    apply_mesh(
        FemMesh::demo_hex8(),
        &PathBuf::from("part.msh"),
        true,
        &mut model,
        &mut status,
        &mut version,
        &mut camera_fit,
        &mut setup,
    );

    assert_eq!(setup.boundary_conditions.len(), 1);
    assert_eq!(model.meshes.len(), 2);
}

#[test]
fn mesh_contact_pair_resolves_tutorial_node_surface_groups() {
    let mut model = FemModel::demo_hex8();
    model.meshes[0].node_sets.push(fem_core::FemNodeSet {
        name: "slave".to_string(),
        nodes: vec![NodeId(0)],
    });
    model.meshes[0]
        .surface_sets
        .push(fem_core::FemSurfaceSet::new("master"));

    let count = merge_mesh_contact_pairs(
        &mut model,
        0,
        vec![hecmw::HecmwContactPairDefinition {
            name: "CP1".to_string(),
            pair_type: hecmw::HecmwContactPairType::NodeSurface,
            slave_group_name: "SLAVE".to_string(),
            master_surface_name: "MASTER".to_string(),
        }],
    );

    assert_eq!(count, 1);
    assert_eq!(model.contacts[0].name, "CP1");
    assert_eq!(model.contacts[0].master, fem_core::SurfaceSetRef::new(0, 0));
    assert_eq!(
        model.contacts[0].slave,
        fem_core::ContactSlaveRef::Nodes(fem_core::NodeSetRef::new(0, 0))
    );
    assert_eq!(
        model.contacts[0].contact_type,
        fem_core::ContactType::SmallSliding
    );
}

#[test]
fn selected_nodes_remain_partitioned_by_mesh() {
    let selection = SelectionState {
        targets: vec![
            FemEntityRef::node(0, NodeId(7)),
            FemEntityRef::node(1, NodeId(7)),
        ],
        ..Default::default()
    };

    let grouped = selected_nodes_by_mesh(&selection);

    assert_eq!(grouped.get(&0), Some(&vec![NodeId(7)]));
    assert_eq!(grouped.get(&1), Some(&vec![NodeId(7)]));
}

#[test]
fn load_preview_direction_tracks_the_sign_but_hides_zero_loads() {
    assert_eq!(signed_preview_direction(Vec3::X, 12.0), Vec3::X);
    assert_eq!(signed_preview_direction(Vec3::X, -12.0), Vec3::NEG_X);
    assert_eq!(signed_preview_direction(Vec3::X, 0.0), Vec3::ZERO);
}

#[test]
fn surface_growth_hint_keeps_face_and_element_meanings_distinct() {
    assert_eq!(
        surface_selection_hint(SelectionLevel::Face, SurfaceSelectionMode::Smooth),
        "Face Smooth = connected curved surface patch"
    );
    assert_eq!(
        surface_selection_hint(SelectionLevel::Element, SurfaceSelectionMode::Smooth),
        "Element Smooth = whole elements behind curved patch"
    );
}

#[test]
fn surface_growth_controls_only_apply_to_face_and_element() {
    assert!(!supports_surface_growth(SelectionLevel::Node));
    assert!(!supports_surface_growth(SelectionLevel::Edge));
    assert!(supports_surface_growth(SelectionLevel::Face));
    assert!(supports_surface_growth(SelectionLevel::Element));
}

#[test]
fn selection_guide_starts_open_and_names_every_modifier_operation() {
    assert!(SelectionGuideState::default().expanded);
    assert_eq!(SELECTION_GUIDE_TEXT.lines().count(), 9);
    assert!(!SELECTION_GUIDE_TEXT.contains('\\'));
    assert!(SELECTION_GUIDE_TEXT.contains("Double click"));
    assert!(SELECTION_GUIDE_TEXT.contains("Triple click"));
    assert!(
        selection_operation_hint(SelectionOperation::Replace)
            .0
            .contains("REPLACE")
    );
    assert!(
        selection_operation_hint(SelectionOperation::Add)
            .0
            .contains("ADD")
    );
    assert!(
        selection_operation_hint(SelectionOperation::Toggle)
            .0
            .contains("TOGGLE")
    );
    assert!(
        selection_operation_hint(SelectionOperation::Remove)
            .0
            .contains("REMOVE")
    );
}

#[test]
fn element_surface_growth_keeps_element_targets_but_highlights_faces() {
    let model = FemModel::demo_hex8();
    let face = model.meshes[0].cached_boundary_faces()[0].clone();
    let element = face.element.expect("a solid boundary face has an owner");
    let hit = SelectionHit::new(FemEntityRef::element(0, element), Vec3::ZERO, 0.0)
        .with_surface(face.id, face.element);

    let mut app = App::new();
    app.insert_resource(model);
    app.insert_resource(HoverResult {
        entity: None,
        hit: Some(hit),
    });
    app.insert_resource(SurfaceSelectionSettings {
        mode: SurfaceSelectionMode::Coplanar,
    });
    app.init_resource::<HoverPreviewTargets>();
    app.add_systems(Update, update_hover_preview_group);

    app.update();

    let preview = app.world().resource::<HoverPreviewTargets>();
    assert!(
        preview
            .targets
            .iter()
            .all(|target| matches!(target.entity, FemEntityId::Element(_)))
    );
    assert!(
        preview
            .highlight_targets
            .iter()
            .all(|target| matches!(target.entity, FemEntityId::Face(_)))
    );
    assert!(!preview.targets.is_empty());
    assert!(!preview.highlight_targets.is_empty());
}

#[test]
fn edge_hover_previews_a_continuous_feature_chain() {
    let mesh = FemMesh::new(
        vec![
            FemNode::new(NodeId(0), Vec3::new(0.0, 0.0, 0.0)),
            FemNode::new(NodeId(1), Vec3::new(1.0, 0.0, 0.0)),
            FemNode::new(NodeId(2), Vec3::new(1.0, 1.0, 0.0)),
            FemNode::new(NodeId(3), Vec3::new(0.0, 1.0, 0.0)),
            FemNode::new(NodeId(4), Vec3::new(2.0, 0.0, 0.0)),
            FemNode::new(NodeId(5), Vec3::new(2.0, 1.0, 0.0)),
        ],
        vec![
            FemElement::new(
                ElementId(0),
                ElementType::ShellQuad4,
                vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
            ),
            FemElement::new(
                ElementId(1),
                ElementType::ShellQuad4,
                vec![NodeId(1), NodeId(4), NodeId(5), NodeId(2)],
            ),
        ],
    );
    let seed = mesh
        .cached_boundary_edges()
        .iter()
        .find(|edge| edge.nodes.contains(&NodeId(0)) && edge.nodes.contains(&NodeId(1)))
        .expect("bottom-left edge")
        .id;
    let model = FemModel::single_mesh("shells", mesh);
    let hit = SelectionHit::new(FemEntityRef::edge(0, seed), Vec3::ZERO, 0.0);

    let mut app = App::new();
    app.insert_resource(model);
    app.insert_resource(HoverResult {
        entity: None,
        hit: Some(hit),
    });
    app.insert_resource(SurfaceSelectionSettings {
        mode: SurfaceSelectionMode::Smooth,
    });
    app.init_resource::<HoverPreviewTargets>();
    app.add_systems(Update, update_hover_preview_group);

    app.update();

    let preview = app.world().resource::<HoverPreviewTargets>();
    assert_eq!(preview.targets.len(), 2);
    assert_eq!(preview.highlight_targets, preview.targets);
    assert!(
        preview
            .targets
            .iter()
            .all(|target| matches!(target.entity, FemEntityId::Edge(_)))
    );
}
