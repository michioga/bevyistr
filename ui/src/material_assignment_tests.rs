use super::*;
use crate::layout::{UndoInProgress, UndoStack, push_undo_before_setup_change, undo_redo_system};
use crate::material_assignment::{MaterialViewportHover, material_assignment_click};
use fem_core::UiKeyboardState;
use fem_core::{AnalysisSetup, FemElementSet, FemMesh, SectionKind};

fn app() -> App {
    let mut model = FemModel::demo_hex8();
    let mut second = FemMesh::demo_hex8();
    second.element_sets.push(FemElementSet {
        name: "ALL".into(),
        elements: vec![fem_core::ElementId(0)],
    });
    model.add_mesh("second", second);
    let mut setup = AnalysisSetup::default();
    setup.add_material("STEEL", Some(210e9), Some(0.3), Some(7850.0));
    setup.add_material("AL", Some(69e9), Some(0.33), Some(2700.0));
    let mut app = App::new();
    app.insert_resource(model)
        .insert_resource(setup)
        .insert_resource(SidebarPage::Materials)
        .init_resource::<SelectedMaterialForSection>()
        .insert_resource(MaterialLibraryState::from_path(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-data/materials.toml"),
        ))
        .insert_resource(fem_core::ViewportTool::MaterialAssignment)
        .init_resource::<fem_core::UiPointerState>()
        .init_resource::<MaterialViewportHover>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<SelectedEgrp>()
        .init_resource::<SelectedSectionType>()
        .init_resource::<FemModelVersion>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<UiKeyboardState>()
        .init_resource::<UndoStack>()
        .init_resource::<UndoInProgress>()
        .add_systems(Startup, |mut commands: Commands| {
            commands
                .spawn(Node::default())
                .with_children(spawn_materials_ui);
        })
        .add_systems(
            Update,
            (
                material_assignment_click,
                egrp_select_button_system,
                crate::material_library::material_library_system,
                material_select_button_system,
                rebuild_section_def_panel,
                add_section_button_system,
                update_material_workflow,
                push_undo_before_setup_change,
                undo_redo_system,
            )
                .chain(),
        );
    app.update();
    app
}

fn click(app: &mut App, name: &str) {
    let entity = app
        .world_mut()
        .query::<(Entity, &Name)>()
        .iter(app.world())
        .find(|(_, n)| n.as_str() == name)
        .unwrap()
        .0;
    app.world_mut()
        .entity_mut(entity)
        .insert(Interaction::Pressed);
    app.update();
    if let Ok(mut entity) = app.world_mut().get_entity_mut(entity) {
        entity.insert(Interaction::None);
    }
}

#[test]
fn viewport_target_then_library_then_confirmation_is_one_undoable_change() {
    let mut app = app();
    app.world_mut().resource_mut::<MaterialViewportHover>().0 = Some(1);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .reset_all();
    assert_eq!(
        app.world()
            .resource::<SelectedEgrp>()
            .0
            .as_ref()
            .unwrap()
            .mesh_index,
        1
    );
    assert!(
        app.world()
            .resource::<SelectedMaterialForSection>()
            .0
            .is_none()
    );
    click(&mut app, "Library_TI6AL4V");
    click(&mut app, "AddSectionButton"); // units not chosen
    assert_eq!(app.world().resource::<AnalysisSetup>().materials.len(), 2);
    assert!(app.world().resource::<AnalysisSetup>().sections.is_empty());
    click(&mut app, "LibraryUnits_Millimetres");
    assert_eq!(app.world().resource::<AnalysisSetup>().materials.len(), 2);
    click(&mut app, "AddSectionButton");
    assert_eq!(app.world().resource::<AnalysisSetup>().materials.len(), 3);
    assert_eq!(
        app.world().resource::<AnalysisSetup>().materials[2].density,
        Some(4.42e-9)
    );
    assert_eq!(
        app.world().resource::<AnalysisSetup>().sections[0].material_name,
        "TI6AL4V"
    );
    assert_eq!(
        app.world().resource::<AnalysisSetup>().sections[0].mesh_index,
        1
    );
    assert_eq!(app.world().resource::<UndoStack>().undo.len(), 1);
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::KeyZ);
    app.update();
    assert_eq!(app.world().resource::<AnalysisSetup>().materials.len(), 2);
    assert!(app.world().resource::<AnalysisSetup>().sections.is_empty());
}

#[test]
fn target_change_and_escape_cancel_pending_material_without_mutating_setup() {
    let mut app = app();
    click(&mut app, "Assignment_0_WHOLE");
    click(&mut app, "Library_STEEL");
    click(&mut app, "LibraryUnits_Metres");
    assert!(
        app.world()
            .resource::<MaterialLibraryState>()
            .draft()
            .is_some()
    );
    click(&mut app, "Assignment_1_WHOLE");
    assert!(
        app.world()
            .resource::<MaterialLibraryState>()
            .draft()
            .is_none()
    );
    assert!(
        app.world()
            .resource::<SelectedMaterialForSection>()
            .0
            .is_none()
    );
    click(&mut app, "Library_TI6AL4V");
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert!(app.world().resource::<SelectedEgrp>().0.is_none());
    assert!(
        app.world()
            .resource::<MaterialLibraryState>()
            .draft()
            .is_none()
    );
    assert_eq!(app.world().resource::<AnalysisSetup>().materials.len(), 2);
    assert!(app.world().resource::<AnalysisSetup>().sections.is_empty());
    for node in app
        .world_mut()
        .query_filtered::<&Node, With<MaterialAfterTarget>>()
        .iter(app.world())
    {
        assert_eq!(node.display, Display::None);
    }
}

#[test]
fn pointer_over_ui_cannot_change_viewport_target() {
    let mut app = app();
    click(&mut app, "Assignment_1_WHOLE");
    app.world_mut().resource_mut::<MaterialViewportHover>().0 = Some(0);
    app.world_mut()
        .resource_mut::<fem_core::UiPointerState>()
        .over_ui = true;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert_eq!(
        app.world()
            .resource::<SelectedEgrp>()
            .0
            .as_ref()
            .unwrap()
            .mesh_index,
        1
    );
}

#[test]
fn target_is_explicit_mesh_scoped_and_assignment_is_undoable() {
    let mut app = app();
    click(&mut app, "AddSectionButton");
    assert!(app.world().resource::<AnalysisSetup>().sections.is_empty());
    click(&mut app, "Assignment_1_WHOLE");
    click(&mut app, "MatSel_STEEL");
    click(&mut app, "AddSectionButton");
    let sections = &app.world().resource::<AnalysisSetup>().sections;
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].mesh_index, 1);
    assert_eq!(sections[0].material_name, "STEEL");
    app.update();
    let undo_count = app.world().resource::<UndoStack>().undo.len();
    click(&mut app, "AddSectionButton");
    assert_eq!(app.world().resource::<UndoStack>().undo.len(), undo_count);
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::KeyZ);
    app.update();
    assert!(app.world().resource::<AnalysisSetup>().sections.is_empty());
}

#[test]
fn a_named_all_group_is_distinct_from_whole_part_and_reload_clears_target() {
    let mut app = app();
    click(&mut app, "Assignment_1_GROUP_ALL");
    click(&mut app, "MatSel_STEEL");
    click(&mut app, "AddSectionButton");
    assert_eq!(
        app.world().resource::<AnalysisSetup>().sections[0]
            .element_set_name
            .as_deref(),
        Some("ALL")
    );
    app.world_mut().resource_mut::<FemModelVersion>().value += 1;
    app.update();
    assert!(app.world().resource::<SelectedEgrp>().0.is_none());
}

#[test]
fn whole_part_reassignment_preserves_geometry_and_covers_unassigned_regions() {
    let mut setup = AnalysisSetup::default();
    setup.add_section(0, "A", None, SectionKind::Solid);
    setup.add_section(
        1,
        "A",
        Some("shells".into()),
        SectionKind::Shell { thickness: 0.75 },
    );
    setup.add_section(
        1,
        "B",
        Some("beams".into()),
        SectionKind::Beam { area: 2.5 },
    );
    let target = AssignmentTarget {
        mesh_index: 1,
        group: None,
    };
    assert!(assign_material(
        &mut setup,
        &target,
        "C",
        SectionKind::Solid
    ));
    assert_eq!(setup.sections[0].material_name, "A");
    assert_eq!(
        setup.sections[1].kind,
        SectionKind::Shell { thickness: 0.75 }
    );
    assert_eq!(setup.sections[2].kind, SectionKind::Beam { area: 2.5 });
    assert!(setup.sections[1..].iter().all(|s| s.material_name == "C"));
    assert!(setup.sections[3].element_set_name.is_none());
    assert!(!assign_material(
        &mut setup,
        &target,
        "C",
        SectionKind::Solid
    ));
}

#[test]
fn missing_or_ambiguous_material_and_hidden_page_cannot_assign() {
    let mut app = app();
    click(&mut app, "Assignment_1_WHOLE");
    click(&mut app, "MatSel_STEEL");
    *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Model;
    click(&mut app, "AddSectionButton");
    assert!(app.world().resource::<AnalysisSetup>().sections.is_empty());
    *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Materials;
    app.world_mut()
        .resource_mut::<AnalysisSetup>()
        .add_material("STEEL", Some(1.0), Some(0.3), None);
    click(&mut app, "AddSectionButton");
    assert!(app.world().resource::<AnalysisSetup>().sections.is_empty());
    app.world_mut()
        .resource_mut::<SelectedMaterialForSection>()
        .0 = Some("MISSING".into());
    click(&mut app, "AddSectionButton");
    assert!(app.world().resource::<AnalysisSetup>().sections.is_empty());
}
