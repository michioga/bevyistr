use super::*;

#[test]
fn clearance_scheduling_integrates_with_ui_and_viewport_phases() {
    let mut app = App::new();
    app.add_plugins((
        interaction::InteractionPlugin,
        selection::SelectionPlugin,
        visualization::VisualizationPlugin,
        crate::UiPlugin,
    ));
    // Initialize the actual system graph without a renderer, window, or file dialogs.
    let mut schedule = app
        .world_mut()
        .resource_mut::<bevy::ecs::schedule::Schedules>()
        .remove(Update)
        .unwrap();
    schedule.initialize(app.world_mut()).unwrap();
}

fn two_demo_parts(offset: Vec3) -> FemModel {
    let mut model = FemModel::single_mesh("A", FemMesh::demo_hex8());
    model.add_mesh("B", FemMesh::demo_hex8());
    assert!(model.translate_part(1, offset));
    model
}

#[test]
fn boundary_collider_reports_cube_clearance() {
    let model = two_demo_parts(Vec3::X * 3.0);
    let colliders: Vec<_> = model
        .parts
        .iter()
        .map(|part| build_boundary_collider(&model.meshes[part.mesh_index]))
        .collect();

    let evaluation = evaluate_selected_part(&colliders, 0, 7).unwrap();
    let report = &evaluation.reports[0];

    assert_eq!(report.kind, ClearanceKind::Separated);
    assert!((report.distance - 1.0).abs() < 1.0e-5);
    assert_eq!(report.other_part, 1);
    assert_eq!(report.checked_version, 7);
    assert_eq!(
        report.part_bounds,
        [model.part_bounds(0).unwrap(), model.part_bounds(1).unwrap()]
    );
}

#[test]
fn boundary_collider_detects_crossing_surfaces() {
    let model = two_demo_parts(Vec3::new(1.2, 0.1, 0.1));
    let colliders: Vec<_> = model
        .parts
        .iter()
        .map(|part| build_boundary_collider(&model.meshes[part.mesh_index]))
        .collect();

    let evaluation = evaluate_selected_part(&colliders, 0, 1).unwrap();
    let report = &evaluation.reports[0];

    assert_eq!(report.kind, ClearanceKind::Intersecting);
    assert_eq!(report.distance, 0.0);
}

#[test]
fn boundary_collider_detects_full_containment() {
    let mut inner = FemMesh::demo_hex8();
    for node in &mut inner.nodes {
        node.position *= 0.4;
    }
    inner.rebuild_topology_cache();
    let mut model = FemModel::single_mesh("Outer", FemMesh::demo_hex8());
    model.add_mesh("Inner", inner);
    let colliders: Vec<_> = model
        .parts
        .iter()
        .map(|part| build_boundary_collider(&model.meshes[part.mesh_index]))
        .collect();

    let evaluation = evaluate_selected_part(&colliders, 0, 3).unwrap();
    let report = &evaluation.reports[0];

    assert_eq!(report.kind, ClearanceKind::Intersecting);
    assert_eq!(report.distance, 0.0);
}

#[test]
fn clearance_reports_are_reviewed_by_risk_then_distance() {
    let mut model = FemModel::single_mesh("Selected", FemMesh::demo_hex8());
    model.add_mesh("Far", FemMesh::demo_hex8());
    model.add_mesh("Intersecting", FemMesh::demo_hex8());
    model.add_mesh("Near", FemMesh::demo_hex8());
    assert!(model.translate_part(1, Vec3::X * 6.0));
    assert!(model.translate_part(2, Vec3::X * 1.2));
    assert!(model.translate_part(3, Vec3::X * 3.0));
    let colliders: Vec<_> = model
        .parts
        .iter()
        .map(|part| build_boundary_collider(&model.meshes[part.mesh_index]))
        .collect();

    let evaluation = evaluate_selected_part(&colliders, 0, 4).unwrap();

    assert_eq!(evaluation.reports.len(), 3);
    assert_eq!(evaluation.reports[0].kind, ClearanceKind::Intersecting);
    assert_eq!(evaluation.reports[0].other_part, 2);
    assert_eq!(evaluation.reports[1].kind, ClearanceKind::Separated);
    assert_eq!(evaluation.reports[1].other_part, 3);
    assert_eq!(evaluation.reports[2].other_part, 1);
}

#[test]
fn clearance_review_wraps_in_both_directions() {
    let mut model = FemModel::single_mesh("Selected", FemMesh::demo_hex8());
    model.add_mesh("Near", FemMesh::demo_hex8());
    model.add_mesh("Far", FemMesh::demo_hex8());
    assert!(model.translate_part(1, Vec3::X * 3.0));
    assert!(model.translate_part(2, Vec3::X * 5.0));
    let mut state = AssemblyClearanceState::default();

    state.check(&model, 9, Some(0));
    assert_eq!(state.active_report().unwrap().other_part, 1);
    state.navigate(ClearanceReviewAction::Next, &model, 9, Some(0));
    assert_eq!(state.active_report().unwrap().other_part, 2);
    state.navigate(ClearanceReviewAction::Next, &model, 9, Some(0));
    assert_eq!(state.active_report().unwrap().other_part, 1);
    state.navigate(ClearanceReviewAction::Previous, &model, 9, Some(0));
    assert_eq!(state.active_report().unwrap().other_part, 2);
}

fn three_demo_parts() -> FemModel {
    let mut model = two_demo_parts(Vec3::X * 3.0);
    model.add_mesh("C", FemMesh::demo_hex8());
    assert!(model.translate_part(2, Vec3::X * 6.0));
    model
}

#[test]
fn navigating_after_mesh_replacement_discards_old_part_indices() {
    let model = three_demo_parts();
    for replacement in [
        FemModel::default(),
        FemModel::single_mesh("Replacement", FemMesh::demo_hex8()),
        three_demo_parts(),
    ] {
        let mut state = AssemblyClearanceState::default();
        state.check(&model, 9, Some(0));
        state.navigate(ClearanceReviewAction::Next, &replacement, 10, Some(0));

        assert!(state.reports.is_empty());
        assert!(state.active_report().is_none());
        assert!(state.colliders.is_empty());
        assert_eq!(state.collider_version, None);
        assert_eq!(state.message, GEOMETRY_CHANGED);
    }
}

#[test]
fn part_count_change_is_safe_even_before_version_update() {
    let mut model = three_demo_parts();
    let mut state = AssemblyClearanceState::default();
    state.check(&model, 9, Some(0));
    model.parts.truncate(1);

    state.navigate(ClearanceReviewAction::Previous, &model, 9, Some(0));
    assert!(state.active_report().is_none());
    assert_eq!(state.message, GEOMETRY_CHANGED);
}

#[test]
fn changing_or_deselecting_part_keeps_only_collider_cache() {
    let model = three_demo_parts();
    for selection in [Some(1), None] {
        let mut state = AssemblyClearanceState::default();
        state.check(&model, 9, Some(0));
        let cache = state.colliders.as_ptr();

        assert!(state.invalidate_if_stale(&model, 9, selection));
        assert!(state.active_report().is_none());
        assert_eq!(state.message, PART_CHANGED);
        assert_eq!(state.collider_version, Some(9));
        assert_eq!(state.colliders.len(), 3);
        assert_eq!(state.colliders.as_ptr(), cache);

        state.check(&model, 9, Some(1));
        assert_eq!(state.active_report().unwrap().selected_part, 1);
        assert_eq!(state.colliders.as_ptr(), cache);
    }
}

#[test]
fn pose_commit_invalidates_and_recheck_uses_new_geometry() {
    let mut model = two_demo_parts(Vec3::X * 3.0);
    let mut state = AssemblyClearanceState::default();
    state.check(&model, 9, Some(0));
    assert!((state.active_report().unwrap().distance - 1.0).abs() < 1.0e-5);
    assert!(!state.invalidate_if_stale(&model, 9, Some(0)));

    assert!(model.translate_part(1, Vec3::X));
    assert!(state.invalidate_if_stale(&model, 10, Some(0)));
    assert!(state.active_report().is_none());
    state.check(&model, 10, Some(0));
    assert!((state.active_report().unwrap().distance - 2.0).abs() < 1.0e-5);
    assert_eq!(
        state.active_report().unwrap().part_bounds[1],
        model.part_bounds(1).unwrap()
    );
}

#[test]
fn review_pauses_only_while_display_differs_from_real_coordinates() {
    let mut contact = ContactReviewSettings::default();
    assert!(review_pause_reason(false, &contact).is_none());
    assert!(
        review_pause_reason(true, &contact)
            .unwrap()
            .contains("Moving part")
    );
    // An inactive review has no exploded transforms even when its slider is nonzero.
    contact.active = true;
    assert!(
        review_pause_reason(false, &contact)
            .unwrap()
            .contains("Review separation")
    );
    contact.separation_percent = 0.0;
    assert!(review_pause_reason(false, &contact).is_none());
    contact.ghost_others = true;
    assert!(review_pause_reason(false, &contact).is_none());
}

fn clearance_app(model: FemModel) -> (App, Entity, Entity, Entity) {
    let mut app = App::new();
    app.insert_resource(model)
        .insert_resource(FemModelVersion { value: 9 })
        .init_resource::<AssemblyEditorState>()
        .init_resource::<ContactReviewSettings>()
        .init_resource::<AssemblyClearanceState>()
        .insert_resource(SidebarPage::Model)
        .add_systems(
            Update,
            (
                assembly_clearance_button_system,
                assembly_clearance_review_button_system,
                sync_assembly_clearance_review,
                (
                    update_assembly_clearance_text,
                    update_assembly_clearance_controls,
                ),
            )
                .chain(),
        );
    let check = app
        .world_mut()
        .spawn((
            Interaction::None,
            AssemblyClearanceButton,
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
        ))
        .id();
    let next = app
        .world_mut()
        .spawn((
            Interaction::None,
            AssemblyClearanceReviewButton {
                action: ClearanceReviewAction::Next,
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
        ))
        .id();
    for parent in [check, next] {
        app.world_mut()
            .spawn((TextColor(TEXT_MAIN), ChildOf(parent)));
    }
    let status = app
        .world_mut()
        .spawn((Text::new(""), TextColor(TEXT_MAIN), AssemblyClearanceText))
        .id();
    (app, check, next, status)
}

fn press(app: &mut App, entity: Entity) {
    app.world_mut()
        .entity_mut(entity)
        .insert(Interaction::Pressed);
    app.update();
    app.world_mut().entity_mut(entity).insert(Interaction::None);
}

#[test]
fn controls_enable_only_available_checks_and_review_pairs() {
    let (mut app, check, next, status) = clearance_app(three_demo_parts());
    app.update();
    assert_eq!(
        app.world().get::<BackgroundColor>(check).unwrap().0,
        BUTTON_NORMAL
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(next).unwrap().0,
        BUTTON_DISABLED
    );

    press(&mut app, check);
    assert_eq!(
        app.world().get::<BackgroundColor>(next).unwrap().0,
        BUTTON_NORMAL
    );
    press(&mut app, next);
    assert_eq!(
        app.world()
            .resource::<AssemblyClearanceState>()
            .selected_report,
        Some(1)
    );

    app.world_mut()
        .insert_resource(FemModel::single_mesh("New", FemMesh::demo_hex8()));
    app.world_mut().resource_mut::<FemModelVersion>().bump();
    press(&mut app, next);
    assert!(
        app.world()
            .resource::<AssemblyClearanceState>()
            .active_report()
            .is_none()
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(check).unwrap().0,
        BUTTON_DISABLED
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(next).unwrap().0,
        BUTTON_DISABLED
    );
    assert!(
        app.world()
            .get::<Text>(status)
            .unwrap()
            .0
            .contains("Add Mesh")
    );
    press(&mut app, check);
    assert!(
        app.world()
            .resource::<AssemblyClearanceState>()
            .colliders
            .is_empty()
    );
}

#[test]
fn one_pair_needs_no_navigation_and_no_selection_cannot_check() {
    let (mut app, check, next, status) = clearance_app(two_demo_parts(Vec3::X * 3.0));
    press(&mut app, check);
    assert!(
        app.world()
            .resource::<AssemblyClearanceState>()
            .active_report()
            .is_some()
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(next).unwrap().0,
        BUTTON_DISABLED
    );
    app.world_mut()
        .resource_mut::<AssemblyEditorState>()
        .selected_part = None;
    press(&mut app, check);
    assert!(
        app.world()
            .resource::<AssemblyClearanceState>()
            .active_report()
            .is_none()
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(check).unwrap().0,
        BUTTON_DISABLED
    );
    assert!(
        app.world()
            .get::<Text>(status)
            .unwrap()
            .0
            .starts_with("Select a part")
    );
}

#[test]
fn exploded_review_blocks_queries_and_navigation_without_erasing_valid_results() {
    let (mut app, check, next, status) = clearance_app(three_demo_parts());
    app.world_mut().insert_resource(SidebarPage::Contact);
    app.world_mut()
        .resource_mut::<ContactReviewSettings>()
        .active = true;
    press(&mut app, check);
    assert!(
        app.world()
            .resource::<AssemblyClearanceState>()
            .colliders
            .is_empty()
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(check).unwrap().0,
        BUTTON_DISABLED
    );
    assert!(
        app.world()
            .get::<Text>(status)
            .unwrap()
            .0
            .contains("Review separation")
    );

    app.world_mut()
        .resource_mut::<ContactReviewSettings>()
        .separation_percent = 0.0;
    press(&mut app, check);
    assert_eq!(
        app.world()
            .resource::<AssemblyClearanceState>()
            .selected_report,
        Some(0)
    );

    app.world_mut()
        .resource_mut::<ContactReviewSettings>()
        .separation_percent = 8.0;
    press(&mut app, next);
    assert_eq!(
        app.world()
            .resource::<AssemblyClearanceState>()
            .selected_report,
        Some(0)
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(next).unwrap().0,
        BUTTON_DISABLED
    );
    app.world_mut()
        .resource_mut::<ContactReviewSettings>()
        .separation_percent = 0.0;
    app.update();
    assert_eq!(
        app.world().get::<BackgroundColor>(next).unwrap().0,
        BUTTON_NORMAL
    );
    assert!(
        app.world()
            .get::<Text>(status)
            .unwrap()
            .0
            .starts_with("Clearance:")
    );
}

#[test]
fn selection_change_invalidates_before_next_pair_is_displayed() {
    let (mut app, check, next, status) = clearance_app(three_demo_parts());
    press(&mut app, check);
    app.world_mut()
        .resource_mut::<AssemblyEditorState>()
        .selected_part = Some(1);
    press(&mut app, next);

    assert!(
        app.world()
            .resource::<AssemblyClearanceState>()
            .active_report()
            .is_none()
    );
    assert_eq!(app.world().get::<Text>(status).unwrap().0, PART_CHANGED);
    assert_eq!(
        app.world().get::<BackgroundColor>(next).unwrap().0,
        BUTTON_DISABLED
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(check).unwrap().0,
        BUTTON_NORMAL
    );
}

#[test]
fn hovering_does_not_rebuild_or_mark_clearance_state_changed() {
    let (mut app, check, _, _) = clearance_app(three_demo_parts());
    press(&mut app, check);
    app.update();
    let before = app
        .world()
        .get_resource_ref::<AssemblyClearanceState>()
        .unwrap()
        .last_changed();
    let cache = app
        .world()
        .resource::<AssemblyClearanceState>()
        .colliders
        .as_ptr();
    app.world_mut()
        .resource_mut::<AssemblyEditorState>()
        .hovered_part = Some(1);
    app.update();

    assert_eq!(
        app.world()
            .get_resource_ref::<AssemblyClearanceState>()
            .unwrap()
            .last_changed(),
        before
    );
    assert_eq!(
        app.world()
            .resource::<AssemblyClearanceState>()
            .colliders
            .as_ptr(),
        cache
    );
    assert!(
        app.world()
            .resource::<AssemblyClearanceState>()
            .active_report()
            .is_some()
    );
}
