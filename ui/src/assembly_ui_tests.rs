use super::*;

fn assembly_app() -> App {
    let mut model = FemModel::single_mesh("Selected", fem_core::FemMesh::demo_hex8());
    model.add_mesh("Unchanged", fem_core::FemMesh::demo_hex8());
    model.translate_part(1, Vec3::X * 6.0);
    let mut app = App::new();
    app.insert_resource(model)
        .insert_resource(SidebarPage::Model)
        .init_resource::<AssemblyEditorState>()
        .init_resource::<MeasurementBoxState>()
        .init_resource::<FemModelVersion>()
        .init_resource::<ContactCandidateState>()
        .init_resource::<HoverResult>()
        .init_resource::<HoverPreviewTargets>()
        .init_resource::<SelectionState>()
        .insert_resource(ViewportTool::Selection)
        .add_systems(Startup, |mut commands: Commands| {
            commands
                .spawn((Node::default(), Name::new("AssemblyRoot")))
                .with_children(spawn_assembly_ui);
        })
        .add_systems(
            Update,
            (
                assembly_tool_button_system,
                update_assembly_nudge_visibility,
                assembly_transform_button_system,
                update_assembly_status_text,
            )
                .chain(),
        );
    app.update();
    app
}

fn named_entity(app: &mut App, name: &str) -> Entity {
    let mut query = app.world_mut().query::<(Entity, &Name)>();
    query
        .iter(app.world())
        .find(|(_, n)| n.as_str() == name)
        .unwrap()
        .0
}

fn press(app: &mut App, name: &str) {
    let entity = named_entity(app, name);
    app.world_mut()
        .entity_mut(entity)
        .insert(Interaction::Pressed);
    app.update();
    app.world_mut().entity_mut(entity).insert(Interaction::None);
}

fn positions(app: &App, mesh: usize) -> Vec<Vec3> {
    app.world().resource::<FemModel>().meshes[mesh]
        .nodes
        .iter()
        .map(|node| node.position)
        .collect()
}

fn step_status(app: &mut App, mode: &str) -> String {
    let entity = named_entity(app, &format!("AssemblyStepInfo_{mode}"));
    app.world().get::<Text>(entity).unwrap().0.clone()
}

#[test]
fn axis_controls_pair_opposite_buttons_around_a_noninteractive_label() {
    let mut app = assembly_app();
    for (mode, labels) in [
        (AssemblyGizmoMode::Move, ["X", "Y", "Z"]),
        (AssemblyGizmoMode::Rotate, ["RX", "RY", "RZ"]),
    ] {
        for (axis, label) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().zip(labels) {
            let group = named_entity(&mut app, &format!("AssemblyAxis_{label}"));
            let children = app.world().get::<Children>(group).unwrap();
            assert_eq!(children.len(), 3);
            let left = app
                .world()
                .get::<AssemblyTransformButton>(children[0])
                .unwrap();
            let right = app
                .world()
                .get::<AssemblyTransformButton>(children[2])
                .unwrap();
            assert_eq!(left.action, AssemblyTransformAction::for_axis(mode, -axis));
            assert_eq!(right.action, AssemblyTransformAction::for_axis(mode, axis));
            assert!(app.world().get::<Button>(children[1]).is_none());
            let label_entity = app.world().get::<Children>(children[1]).unwrap()[0];
            assert_eq!(app.world().get::<Text>(label_entity).unwrap().0, label);
            for (index, sign) in [(0, "-"), (2, "+")] {
                let text = app.world().get::<Children>(children[index]).unwrap()[0];
                assert_eq!(app.world().get::<Text>(text).unwrap().0, sign);
            }
            let (_, border) = transform_button_colors(right.action, Interaction::None);
            assert_eq!(
                *app.world().get::<BorderColor>(group).unwrap(),
                BorderColor::all(border)
            );
        }
    }
    let mut buttons = app.world_mut().query::<&AssemblyTransformButton>();
    assert_eq!(buttons.iter(app.world()).count(), 13); // six per mode, plus reset
}

#[test]
fn only_active_mode_controls_take_space_and_each_step_is_retained() {
    let mut app = assembly_app();
    press(&mut app, "AssemblyTool_Move");
    let move_controls = named_entity(&mut app, "AssemblyNudgeControls_Move");
    let rotate_controls = named_entity(&mut app, "AssemblyNudgeControls_Rotate");
    assert_eq!(
        app.world().get::<Node>(move_controls).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().get::<Node>(rotate_controls).unwrap().display,
        Display::None
    );
    {
        let mut sliders = app.world_mut().query::<&mut SliderState>();
        for mut slider in sliders.iter_mut(app.world_mut()) {
            if slider.id == SliderId::AssemblyRotationDegrees {
                slider.value = 7.5;
            }
            if slider.id == SliderId::AssemblyMovePercent {
                slider.value = 2.0;
            }
        }
    }
    press(&mut app, "AssemblyTool_Rotate");
    assert_eq!(
        app.world().get::<Node>(move_controls).unwrap().display,
        Display::None
    );
    assert_eq!(
        app.world().get::<Node>(rotate_controls).unwrap().display,
        Display::Flex
    );
    assert!(step_status(&mut app, "Rotate").contains("Rotate step: 7.5000 deg"));
    press(&mut app, "AssemblyTool_Move");
    assert_eq!(
        app.world().get::<Node>(move_controls).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().get::<Node>(rotate_controls).unwrap().display,
        Display::None
    );
    assert!(step_status(&mut app, "Move").contains("Move step:"));
    let mut sliders = app.world_mut().query::<&SliderState>();
    assert!(
        sliders
            .iter(app.world())
            .any(|s| s.id == SliderId::AssemblyRotationDegrees && s.value == 7.5)
    );
    assert!(
        sliders
            .iter(app.world())
            .any(|s| s.id == SliderId::AssemblyMovePercent && s.value == 2.0)
    );
    // Choosing Move enables both its viewport gizmo and the panel steps.
    assert_eq!(
        *app.world().resource::<ViewportTool>(),
        ViewportTool::Assembly
    );
}

#[test]
fn signed_move_buttons_change_only_the_selected_part_along_the_named_axis() {
    for (axis, label) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().zip(["X", "Y", "Z"]) {
        for (sign, direction) in [("-", -axis), ("+", axis)] {
            let mut app = assembly_app();
            let before = positions(&app, 0);
            let other = positions(&app, 1);
            let step = assembly_reference_size(app.world().resource::<FemModel>(), 0) * 0.01;
            press(&mut app, "AssemblyTool_Move");
            press(&mut app, &format!("AssemblyStep_{sign}{label}"));
            for (before, after) in before.into_iter().zip(positions(&app, 0)) {
                assert!(after.distance(before + direction * step) < 1.0e-6);
            }
            assert_eq!(positions(&app, 1), other);
            assert_eq!(app.world().resource::<FemModelVersion>().value, 1);
        }
    }
}

#[test]
fn signed_rotation_buttons_follow_the_right_hand_rule_about_part_center() {
    for (axis, label) in [Vec3::X, Vec3::Y, Vec3::Z]
        .into_iter()
        .zip(["RX", "RY", "RZ"])
    {
        for (sign, direction) in [("-", -axis), ("+", axis)] {
            let mut app = assembly_app();
            app.world_mut()
                .resource_mut::<FemModel>()
                .translate_part(0, Vec3::new(2.0, 3.0, 4.0));
            let before = positions(&app, 0);
            let other = positions(&app, 1);
            let center = app.world().resource::<FemModel>().part_centroid(0).unwrap();
            press(&mut app, "AssemblyTool_Rotate");
            press(&mut app, &format!("AssemblyStep_{sign}{label}"));
            let rotation = Quat::from_axis_angle(direction, 5.0_f32.to_radians());
            for (before, after) in before.into_iter().zip(positions(&app, 0)) {
                assert!(after.distance(center + rotation * (before - center)) < 1.0e-5);
            }
            assert_eq!(positions(&app, 1), other);
            assert!(
                app.world()
                    .resource::<FemModel>()
                    .part_centroid(0)
                    .unwrap()
                    .distance(center)
                    < 1.0e-5
            );
            assert_eq!(app.world().resource::<FemModelVersion>().value, 1);
        }
    }
}

#[test]
fn hidden_mode_buttons_do_not_apply_a_stale_press() {
    let mut app = assembly_app();
    press(&mut app, "AssemblyTool_Move");
    let before = positions(&app, 0);
    press(&mut app, "AssemblyStep_+RX");
    assert_eq!(positions(&app, 0), before);
    press(&mut app, "AssemblyTool_Rotate");
    press(&mut app, "AssemblyStep_+X");
    assert_eq!(positions(&app, 0), before);
    assert_eq!(app.world().resource::<FemModelVersion>().value, 0);
}

#[test]
fn absent_selection_and_unrelated_pages_disable_nudging() {
    for selection in [None, Some(99)] {
        let mut app = assembly_app();
        press(&mut app, "AssemblyTool_Move");
        let before = positions(&app, 0);
        app.world_mut()
            .resource_mut::<AssemblyEditorState>()
            .selected_part = selection;
        press(&mut app, "AssemblyStep_+X");
        assert_eq!(positions(&app, 0), before);
        assert_eq!(app.world().resource::<FemModelVersion>().value, 0);
    }
    let mut app = assembly_app();
    let before = positions(&app, 0);
    press(&mut app, "AssemblyTool_Move");
    app.world_mut().insert_resource(SidebarPage::Loads);
    press(&mut app, "AssemblyStep_+X");
    assert_eq!(positions(&app, 0), before);
    app.world_mut().insert_resource(SidebarPage::Contact);
    press(&mut app, "AssemblyStep_+X");
    assert_ne!(positions(&app, 0), before);
}

#[test]
fn reset_remains_available_in_both_modes() {
    for mode in ["Move", "Rotate"] {
        let mut app = assembly_app();
        let original = positions(&app, 0);
        press(&mut app, "AssemblyTool_Move");
        press(&mut app, "AssemblyStep_+X");
        press(&mut app, "AssemblyTool_Rotate");
        press(&mut app, "AssemblyStep_+RZ");
        press(&mut app, &format!("AssemblyTool_{mode}"));
        press(&mut app, "AssemblyResetPoseButton");
        for (original, actual) in original.into_iter().zip(positions(&app, 0)) {
            assert!(original.distance(actual) < 1.0e-5);
        }
    }
}

#[test]
fn opposite_signs_and_rotation_share_the_viewport_axis_palette() {
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        for interaction in [
            Interaction::None,
            Interaction::Hovered,
            Interaction::Pressed,
        ] {
            let positive =
                transform_button_colors(AssemblyTransformAction::Translate(axis), interaction);
            assert_eq!(
                positive,
                transform_button_colors(AssemblyTransformAction::Translate(-axis), interaction)
            );
            assert_eq!(
                positive,
                transform_button_colors(AssemblyTransformAction::Rotate(axis), interaction)
            );
            assert_eq!(
                positive,
                transform_button_colors(AssemblyTransformAction::Rotate(-axis), interaction)
            );
        }
    }
}

#[test]
fn position_workflow_stays_contiguous_and_clearance_follows_it() {
    let mut app = assembly_app();
    let root = named_entity(&mut app, "AssemblyRoot");
    let parts = named_entity(&mut app, "AssemblyPartsContainer");
    let controls = named_entity(&mut app, "AssemblyTransformControls");
    let clearance = named_entity(&mut app, "AssemblyClearanceSection");
    let children = app.world().get::<Children>(root).unwrap();
    let position = |entity| children.iter().position(|child| child == entity).unwrap();
    assert!(position(parts) < position(controls));
    assert!(position(controls) < position(clearance));
    for name in [
        "AssemblyToolBar",
        "AssemblyNudgeControls_Move",
        "AssemblyNudgeControls_Rotate",
        "AssemblyPoseActions",
    ] {
        let entity = named_entity(&mut app, name);
        assert_eq!(
            app.world().get::<ChildOf>(entity).unwrap().parent(),
            controls
        );
    }
    let mut texts = app.world_mut().query::<&Text>();
    assert!(
        !texts
            .iter(app.world())
            .any(|text| text.0.contains("Edit in viewport"))
    );
}

#[test]
fn select_hides_position_actions_and_never_nudges_a_part() {
    let mut app = assembly_app();
    let original = positions(&app, 0);
    for phase in [0, 1] {
        if phase == 1 {
            press(&mut app, "AssemblyTool_Rotate");
            press(&mut app, "AssemblyTool_Select");
        }
        assert_eq!(
            *app.world().resource::<ViewportTool>(),
            ViewportTool::Selection
        );
        for name in [
            "AssemblyNudgeControls_Move",
            "AssemblyNudgeControls_Rotate",
            "AssemblyPoseActions",
        ] {
            let entity = named_entity(&mut app, name);
            assert_eq!(
                app.world().get::<Node>(entity).unwrap().display,
                Display::None
            );
        }
        for name in [
            "AssemblyStep_+X",
            "AssemblyStep_+RX",
            "AssemblyResetPoseButton",
        ] {
            press(&mut app, name);
        }
        assert_eq!(positions(&app, 0), original);
        assert_eq!(app.world().resource::<FemModelVersion>().value, 0);
    }
}

#[test]
fn a_single_tool_choice_activates_matching_viewport_mode_and_clears_old_selection() {
    use fem_core::{FemEntityRef, NodeId};
    let mut app = assembly_app();
    let entity = app.world_mut().spawn((Selected, Hovered)).id();
    let target = FemEntityRef::node(0, NodeId(1));
    {
        let mut selection = app.world_mut().resource_mut::<SelectionState>();
        selection.entities.push(entity);
        selection.targets.push(target);
        selection.highlight_targets.push(target);
    }
    app.world_mut()
        .resource_mut::<HoverPreviewTargets>()
        .targets
        .push(target);
    // Re-selecting Select is a no-op and must not erase FEM selection.
    press(&mut app, "AssemblyTool_Select");
    assert_eq!(
        app.world().resource::<SelectionState>().targets,
        vec![target]
    );
    for (name, mode) in [
        ("Move", AssemblyGizmoMode::Move),
        ("Rotate", AssemblyGizmoMode::Rotate),
    ] {
        press(&mut app, &format!("AssemblyTool_{name}"));
        assert_eq!(
            *app.world().resource::<ViewportTool>(),
            ViewportTool::Assembly
        );
        assert_eq!(
            app.world().resource::<AssemblyEditorState>().gizmo_mode,
            mode
        );
        assert!(app.world().resource::<SelectionState>().targets.is_empty());
        assert!(
            app.world()
                .resource::<HoverPreviewTargets>()
                .targets
                .is_empty()
        );
        assert!(app.world().get::<Selected>(entity).is_none());
        assert!(app.world().get::<Hovered>(entity).is_none());
        let mut buttons = app
            .world_mut()
            .query::<(&AssemblyToolButton, &BorderColor)>();
        let active: Vec<_> = buttons
            .iter(app.world())
            .filter(|(_, border)| **border == BorderColor::all(ACTIVE_BORDER))
            .map(|(button, _)| button.choice)
            .collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].gizmo_mode(), Some(mode));
    }
}

#[test]
fn reselecting_the_active_tool_preserves_exact_input_and_step_values() {
    let mut app = assembly_app();
    press(&mut app, "AssemblyTool_Move");
    {
        let mut measurement = app.world_mut().resource_mut::<MeasurementBoxState>();
        measurement.begin_assembly_translation(0, Vec3::X);
        measurement.commit_translation(2.5);
    }
    press(&mut app, "AssemblyTool_Move");
    assert_eq!(app.world().resource::<MeasurementBoxState>().value, 2.5);
    assert!(
        app.world()
            .resource::<MeasurementBoxState>()
            .target
            .is_some()
    );
    press(&mut app, "AssemblyTool_Rotate");
    assert!(
        app.world()
            .resource::<MeasurementBoxState>()
            .target
            .is_none()
    );
    press(&mut app, "AssemblyTool_Select");
    assert_eq!(
        *app.world().resource::<ViewportTool>(),
        ViewportTool::Selection
    );
}

#[test]
fn hidden_workflow_cannot_activate_a_position_tool() {
    let mut app = assembly_app();
    app.world_mut().insert_resource(SidebarPage::Loads);
    press(&mut app, "AssemblyTool_Move");
    assert_eq!(
        *app.world().resource::<ViewportTool>(),
        ViewportTool::Selection
    );
    press(&mut app, "AssemblyStep_+X");
    assert_eq!(app.world().resource::<FemModelVersion>().value, 0);
    app.world_mut().insert_resource(SidebarPage::Contact);
    press(&mut app, "AssemblyTool_Move");
    assert_eq!(
        *app.world().resource::<ViewportTool>(),
        ViewportTool::Assembly
    );
}
