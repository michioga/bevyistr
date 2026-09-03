use super::*;
use bevy::mesh::VertexAttributeValues;
use fem_core::{AnalysisSetup, ElementId, SectionKind};

#[test]
fn surface_colors_follow_element_assignments_without_changing_geometry() {
    let mut mesh = FemMesh::demo_hex8();
    let mut extra = FemMesh::demo_hex8();
    for node in &mut extra.nodes {
        node.id.0 += 8;
        node.position += Vec3::X * 4.0;
    }
    for element in &mut extra.elements {
        element.id.0 += 1;
        for node in &mut element.nodes {
            node.0 += 8;
        }
    }
    mesh.nodes.extend(extra.nodes);
    mesh.elements.extend(extra.elements);
    mesh.rebuild_topology_cache();
    let mut assignments = std::collections::BTreeMap::new();
    assignments.insert(ElementId(0), MaterialIdentity::Assigned("STEEL".into()));
    // Second element intentionally has no assignment.
    let plain = build_part_surface_mesh(&mesh).unwrap();
    let colored = build_material_surface_mesh(&mesh, Some(&assignments)).unwrap();
    assert_eq!(
        plain
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3(),
        colored
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
    );
    let VertexAttributeValues::Float32x4(colors) =
        colored.attribute(Mesh::ATTRIBUTE_COLOR).unwrap()
    else {
        panic!("vertex colors")
    };
    let positions = colored
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .unwrap()
        .as_float3()
        .unwrap();
    assert_eq!(colors.len(), positions.len());
    for (position, color) in positions.iter().zip(colors) {
        let expected = if position[0] < 2.0 {
            crate::material_identity_color("STEEL")
        } else {
            crate::UNASSIGNED_MATERIAL_COLOR
        };
        assert_eq!(*color, expected.to_linear().to_f32_array());
    }
}

fn render_app() -> App {
    let mut model = FemModel::demo_hex8();
    model.add_mesh("second", FemMesh::demo_hex8());
    let mut setup = AnalysisSetup::default();
    setup.add_material("STEEL", Some(210e9), Some(0.3), Some(7850.0));
    for index in [0, 1] {
        setup.add_section(index, "STEEL", None, SectionKind::Solid);
    }
    let mut app = App::new();
    app.insert_resource(model)
        .insert_resource(setup)
        .init_resource::<fem_core::FemModelVersion>()
        .init_resource::<MaterialColorMode>()
        .init_resource::<VisualizationSettings>()
        .init_resource::<SelectionState>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_systems(Startup, spawn_demo_mesh)
        .add_systems(
            Update,
            (
                respawn_elements_on_setup_change,
                restore_selection_on_new_visuals,
                update_visual_layer_visibility,
                apply_visualization_mode,
                update_hover_materials,
            )
                .chain(),
        );
    app.update();
    app
}

fn element_entities(app: &mut App) -> Vec<Entity> {
    app.world_mut()
        .query_filtered::<Entity, With<ElementEntity>>()
        .iter(app.world())
        .collect()
}

#[test]
fn both_parts_use_same_material_and_numeric_edits_do_not_respawn() {
    let mut app = render_app();
    let entities = element_entities(&mut app);
    assert_eq!(entities.len(), 2);
    for &entity in &entities {
        let handle = &app.world().get::<NormalMaterial>(entity).unwrap().0;
        let material = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(handle)
            .unwrap();
        assert_eq!(material.base_color, crate::material_identity_color("STEEL"));
        assert_eq!(material.alpha_mode, AlphaMode::Opaque);
    }
    app.world_mut().resource_mut::<AnalysisSetup>().materials[0].young_modulus = Some(215e9);
    app.update();
    assert_eq!(element_entities(&mut app), entities);
    app.world_mut()
        .resource_mut::<AnalysisSetup>()
        .add_material("UNUSED", Some(1.0), Some(0.3), None);
    app.update();
    assert_eq!(element_entities(&mut app), entities);
    app.world_mut().resource_mut::<AnalysisSetup>().sections[1].material_name = "UNUSED".into();
    app.update();
    assert_ne!(element_entities(&mut app), entities);
}

#[test]
fn selected_color_wins_and_part_color_mode_is_reversible() {
    let mut app = render_app();
    let entity = element_entities(&mut app)[0];
    let target = app.world().get::<Selectable>(entity).unwrap().target;
    {
        let mut selection = app.world_mut().resource_mut::<SelectionState>();
        selection.entities.push(entity);
        selection.targets.push(target);
        selection.highlight_targets.push(target);
    }
    app.world_mut().entity_mut(entity).insert(Selected);
    app.world_mut().resource_mut::<VisualizationSettings>().mode = VisualizationMode::Flat;
    app.update();
    assert_eq!(
        app.world()
            .get::<MeshMaterial3d<StandardMaterial>>(entity)
            .unwrap()
            .0,
        app.world().get::<SelectedMaterial>(entity).unwrap().0
    );
    *app.world_mut().resource_mut::<MaterialColorMode>() = MaterialColorMode::Part;
    app.update();
    let entities = element_entities(&mut app);
    let colors: Vec<_> = entities
        .iter()
        .map(|entity| {
            let handle = &app.world().get::<NormalMaterial>(*entity).unwrap().0;
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(handle)
                .unwrap()
                .base_color
        })
        .collect();
    assert_ne!(colors[0], colors[1]);
    let rebound = app.world().resource::<SelectionState>().entities.clone();
    assert_eq!(rebound.len(), 1);
    assert!(app.world().get::<Selected>(rebound[0]).is_some());
    assert_ne!(rebound[0], entity);
    for (layer, visibility) in app
        .world_mut()
        .query::<(&VisualLayer, &Visibility)>()
        .iter(app.world())
    {
        assert_eq!(
            *visibility == Visibility::Visible,
            layer.visible_in(VisualizationMode::Flat)
        );
    }
    *app.world_mut().resource_mut::<MaterialColorMode>() = MaterialColorMode::Material;
    app.update();
    for entity in element_entities(&mut app) {
        let handle = &app.world().get::<NormalMaterial>(entity).unwrap().0;
        assert_eq!(
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(handle)
                .unwrap()
                .base_color,
            crate::material_identity_color("STEEL")
        );
    }
}
