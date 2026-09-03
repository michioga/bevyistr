use super::*;
use crate::layout::{UndoInProgress, UndoStack, push_undo_before_setup_change, undo_redo_system};
use crate::material_library::{MaterialLibraryState, material_library_system};
use crate::materials_ui::{
    material_select_button_system, rebuild_section_def_panel, spawn_materials_ui,
};

fn editor_app() -> App {
    let mut setup = AnalysisSetup::default();
    setup.add_material("STEEL", Some(210_000.0), Some(0.3), Some(7.85e-9));
    setup.add_material("AL", Some(69_000.0), Some(0.33), None);
    setup.add_section(0, "STEEL", None, fem_core::SectionKind::Solid);
    setup.add_section(1, "STEEL", None, fem_core::SectionKind::Solid);
    let mut app = App::new();
    app.insert_resource(setup)
        .insert_resource(SidebarPage::Materials)
        .init_resource::<FemModelVersion>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputFocus>()
        .init_resource::<UiKeyboardState>()
        .insert_resource(SelectedMaterialForSection(Some("STEEL".into())))
        .insert_resource(crate::materials_ui::SelectedEgrp(Some(
            crate::materials_ui::AssignmentTarget {
                mesh_index: 0,
                group: None,
            },
        )))
        .init_resource::<MaterialEditorState>()
        .insert_resource(MaterialLibraryState::from_path(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-data/materials.toml"),
        ))
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
                material_library_system,
                material_select_button_system,
                material_numeric_input_system,
                rebuild_section_def_panel,
                push_undo_before_setup_change,
                undo_redo_system,
            )
                .chain(),
        );
    app.update();
    app
}

fn input_entity(app: &mut App, field: MaterialField) -> Entity {
    app.world_mut()
        .query::<(Entity, &MaterialValueInput)>()
        .iter(app.world())
        .find(|(_, input)| input.0 == field)
        .unwrap()
        .0
}

fn text(app: &mut App, field: MaterialField) -> String {
    let entity = input_entity(app, field);
    editable_value(app.world().get::<EditableText>(entity).unwrap())
}

fn type_value(app: &mut App, field: MaterialField, value: &str) -> Entity {
    let entity = input_entity(app, field);
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(entity, bevy::input_focus::FocusCause::Navigated);
    app.world_mut()
        .get_mut::<EditableText>(entity)
        .unwrap()
        .editor_mut()
        .set_text(value);
    app.update();
    entity
}

fn key(app: &mut App, code: KeyCode) {
    app.world_mut()
        .resource_mut::<UiKeyboardState>()
        .text_editing = false;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(code);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
}

fn named_entity(app: &mut App, name: &str) -> Entity {
    app.world_mut()
        .query::<(Entity, &Name)>()
        .iter(app.world())
        .find(|(_, item)| item.as_str() == name)
        .unwrap()
        .0
}

fn click(app: &mut App, name: &str) {
    let entity = named_entity(app, name);
    app.world_mut()
        .entity_mut(entity)
        .insert(Interaction::Pressed);
    app.update();
    if let Ok(mut entity) = app.world_mut().get_entity_mut(entity) {
        entity.insert(Interaction::None);
    }
}

#[test]
fn material_domains_reject_invalid_values_but_allow_auxetic_poisson_ratios() {
    for field in MaterialField::ALL {
        for invalid in ["abc", "NaN", "inf", "-inf", "1e999", "1e-999", "--1"] {
            assert!(
                parse_material_value(field, invalid).is_err(),
                "{field:?} {invalid}"
            );
        }
        assert!(parse_material_value(field, "1e-90").is_err());
    }
    for field in [MaterialField::Young, MaterialField::Density] {
        for invalid in ["-1", "0", "-0"] {
            assert!(parse_material_value(field, invalid).is_err());
        }
    }
    for invalid in ["-1", "-1.1", "0.5", "1", ""] {
        assert!(parse_material_value(MaterialField::Poisson, invalid).is_err());
    }
    assert_eq!(
        parse_material_value(MaterialField::Poisson, "-0.2"),
        Ok(Some(-0.2))
    );
    assert_eq!(
        parse_material_value(MaterialField::Poisson, "0"),
        Ok(Some(0.0))
    );
    assert_eq!(
        parse_material_value(MaterialField::Density, " 7.85e-9 "),
        Ok(Some(7.85e-9))
    );
    assert_eq!(parse_material_value(MaterialField::Density, " "), Ok(None));
    assert!(parse_material_value(MaterialField::Young, " ").is_err());
}

#[test]
fn displayed_values_round_trip_without_a_fixed_decimal_limit() {
    for value in [
        210_000.0,
        2.05e11,
        7.85e-9,
        0.33333334,
        5.0e-8,
        1.2345679e-28,
        f32::MIN_POSITIVE,
        f32::MAX,
    ] {
        assert_eq!(
            value_text(Some(value)).parse::<f32>().unwrap().to_bits(),
            value.to_bits()
        );
    }
    assert_eq!(value_text(None), "");
}

#[test]
fn imported_values_are_selected_without_converting_or_filling_missing_properties() {
    let mut app = editor_app();
    assert_eq!(
        app.world()
            .resource::<SelectedMaterialForSection>()
            .0
            .as_deref(),
        Some("STEEL")
    );
    assert_eq!(text(&mut app, MaterialField::Young), "210000");
    assert_eq!(
        text(&mut app, MaterialField::Density)
            .parse::<f32>()
            .unwrap(),
        7.85e-9
    );
    click(&mut app, "MatSel_AL");
    assert_eq!(text(&mut app, MaterialField::Density), "");
    assert_eq!(
        app.world().resource::<AnalysisSetup>().materials[1].density,
        None
    );
    assert!(app.world().resource::<UndoStack>().undo.is_empty());
}

#[test]
fn only_enter_commits_and_only_the_selected_property_is_changed() {
    let mut app = editor_app();
    let before = app.world().resource::<AnalysisSetup>().clone();
    type_value(&mut app, MaterialField::Young, "215123.45");
    assert_eq!(
        app.world().resource::<AnalysisSetup>().materials,
        before.materials
    );
    key(&mut app, KeyCode::Enter);
    let setup = app.world().resource::<AnalysisSetup>();
    assert_eq!(setup.materials[0].young_modulus, Some(215123.45));
    assert_eq!(
        setup.materials[0].poisson_ratio,
        before.materials[0].poisson_ratio
    );
    assert_eq!(setup.materials[0].density, before.materials[0].density);
    assert_eq!(setup.materials[1], before.materials[1]);
    assert_eq!(setup.sections, before.sections);
    assert!(app.world().resource::<InputFocus>().get().is_none());
    assert!(app.world().resource::<UiKeyboardState>().text_editing);
    assert_eq!(app.world().resource::<UndoStack>().undo.len(), 1);
}

#[test]
fn invalid_enter_does_not_modify_the_model_and_escape_restores_the_value() {
    let mut app = editor_app();
    let entity = type_value(&mut app, MaterialField::Poisson, "0.5");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.world().resource::<AnalysisSetup>().materials[0].poisson_ratio,
        Some(0.3)
    );
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(entity));
    assert!(
        app.world()
            .resource::<MaterialEditorState>()
            .error
            .is_some()
    );
    key(&mut app, KeyCode::Escape);
    assert_eq!(text(&mut app, MaterialField::Poisson), "0.3");
    assert!(
        app.world()
            .resource::<MaterialEditorState>()
            .error
            .is_none()
    );
    assert!(app.world().resource::<UndoStack>().undo.is_empty());
}

#[test]
fn density_can_be_unset_without_affecting_elastic_constants() {
    let mut app = editor_app();
    type_value(&mut app, MaterialField::Density, "");
    key(&mut app, KeyCode::Enter);
    let material = &app.world().resource::<AnalysisSetup>().materials[0];
    assert_eq!(material.density, None);
    assert_eq!(material.young_modulus, Some(210_000.0));
    assert_eq!(material.poisson_ratio, Some(0.3));
}

#[test]
fn switching_material_or_page_cannot_commit_a_stale_draft() {
    for change_page in [false, true] {
        let mut app = editor_app();
        let before = app.world().resource::<AnalysisSetup>().materials.clone();
        type_value(&mut app, MaterialField::Young, "123");
        if change_page {
            *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Solve;
        } else {
            app.world_mut()
                .resource_mut::<SelectedMaterialForSection>()
                .0 = Some("AL".to_string());
        }
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.world().resource::<AnalysisSetup>().materials, before);
        assert!(app.world().resource::<InputFocus>().get().is_none());
        assert_eq!(
            text(&mut app, MaterialField::Young),
            if change_page { "210000" } else { "69000" }
        );
    }
}

#[test]
fn reload_deletion_and_external_changes_invalidate_pending_input() {
    for action in 0..3 {
        let mut app = editor_app();
        type_value(&mut app, MaterialField::Young, "123");
        match action {
            0 => app.world_mut().resource_mut::<FemModelVersion>().bump(),
            1 => {
                app.world_mut()
                    .resource_mut::<AnalysisSetup>()
                    .materials
                    .remove(0);
            }
            _ => {
                app.world_mut().resource_mut::<AnalysisSetup>().materials[0].young_modulus =
                    Some(99.0);
            }
        }
        key(&mut app, KeyCode::Enter);
        assert!(app.world().resource::<InputFocus>().get().is_none());
        let expected = [210_000.0, 69_000.0, 99.0][action];
        assert_eq!(
            app.world().resource::<AnalysisSetup>().materials[0].young_modulus,
            Some(expected)
        );
    }
}

#[test]
fn an_empty_or_ambiguous_material_list_has_no_editable_fields() {
    for duplicate in [false, true] {
        let mut app = editor_app();
        if duplicate {
            app.world_mut()
                .resource_mut::<AnalysisSetup>()
                .add_material("STEEL", Some(123.0), Some(0.2), None);
        } else {
            app.world_mut()
                .resource_mut::<AnalysisSetup>()
                .materials
                .clear();
        }
        app.update();
        for panel in app
            .world_mut()
            .query_filtered::<&Node, With<MaterialFields>>()
            .iter(app.world())
        {
            assert_eq!(panel.display, Display::None);
        }
        let before = app.world().resource::<AnalysisSetup>().materials.clone();
        type_value(&mut app, MaterialField::Young, "123");
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.world().resource::<AnalysisSetup>().materials, before);
    }
}

#[test]
fn an_imported_nonfinite_value_can_be_repaired() {
    let mut app = editor_app();
    app.world_mut().resource_mut::<AnalysisSetup>().materials[0].young_modulus = Some(f32::NAN);
    app.update();
    type_value(&mut app, MaterialField::Young, "2.1e5");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.world().resource::<AnalysisSetup>().materials[0].young_modulus,
        Some(210_000.0)
    );
}

#[test]
fn library_choice_is_only_a_draft_and_project_choice_cancels_it() {
    let mut app = editor_app();
    click(&mut app, "Library_TI6AL4V");
    assert!(
        app.world()
            .resource::<MaterialLibraryState>()
            .draft()
            .is_none()
    );
    click(&mut app, "LibraryUnits_Millimetres");
    assert_eq!(
        app.world()
            .resource::<MaterialLibraryState>()
            .draft()
            .unwrap()
            .density,
        Some(4.42e-9)
    );
    assert_eq!(app.world().resource::<AnalysisSetup>().materials.len(), 2);
    assert!(
        app.world()
            .resource::<SelectedMaterialForSection>()
            .0
            .is_none()
    );
    assert!(app.world().resource::<UndoStack>().undo.is_empty());
    click(&mut app, "MatSel_AL");
    assert!(
        app.world()
            .resource::<MaterialLibraryState>()
            .draft()
            .is_none()
    );
    assert_eq!(text(&mut app, MaterialField::Young), "69000");
    *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Model;
    click(&mut app, "Library_STEEL");
    assert!(
        app.world()
            .resource::<MaterialLibraryState>()
            .selected
            .is_none()
    );
}

#[test]
fn undo_redo_restores_material_constants_and_enter_on_unchanged_value_is_a_noop() {
    let mut app = editor_app();
    type_value(&mut app, MaterialField::Young, "210000");
    key(&mut app, KeyCode::Enter);
    assert!(app.world().resource::<UndoStack>().undo.is_empty());
    type_value(&mut app, MaterialField::Young, "215000");
    key(&mut app, KeyCode::Enter);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    key(&mut app, KeyCode::KeyZ);
    app.update();
    assert_eq!(text(&mut app, MaterialField::Young), "210000");
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    key(&mut app, KeyCode::KeyY);
    app.update();
    assert_eq!(text(&mut app, MaterialField::Young), "215000");
}

#[test]
fn object_material_and_confirmation_follow_reading_order() {
    let mut app = editor_app();
    let selector = named_entity(&mut app, "MaterialSelectorRow");
    let editor = named_entity(&mut app, "MaterialExactEditor");
    let sections = named_entity(&mut app, "SectionDefPanel");
    let parent = app.world().get::<ChildOf>(selector).unwrap().parent();
    for entity in [editor] {
        assert_eq!(app.world().get::<ChildOf>(entity).unwrap().parent(), parent);
    }
    let root = app.world().get::<ChildOf>(parent).unwrap().parent();
    assert_eq!(app.world().get::<ChildOf>(sections).unwrap().parent(), root);
    let target = named_entity(&mut app, "SectionDefEgrpRow");
    let children = app.world().get::<Children>(root).unwrap();
    let positions = [target, parent, sections]
        .map(|entity| children.iter().position(|child| child == entity).unwrap());
    assert!(positions[0] < positions[1] && positions[1] < positions[2]);
}

#[test]
fn material_editor_integrates_with_the_full_ui_schedule() {
    let mut app = App::new();
    app.add_plugins((
        interaction::InteractionPlugin,
        visualization::VisualizationPlugin,
        crate::UiPlugin,
    ));
    app.world_mut().schedule_scope(Update, |world, schedule| {
        schedule
            .initialize(world)
            .expect("UI schedule must have no dependency cycles or conflicting queries");
    });
}
