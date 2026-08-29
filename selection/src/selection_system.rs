use bevy::prelude::*;
use fem_core::{
    ElementId, FemEntityId, FemEntityRef, FemMesh, FemModel, NodeId, SelectionFilter,
    SelectionHit, SelectionLevel, UiKeyboardState, UiPointerState, ViewportTool,
    DEFAULT_FEATURE_EDGE_ANGLE_DEG,
    expand_connected_boundary_edges, expand_connected_boundary_faces, expand_connected_elements,
    expand_connected_feature_edges,
};
use std::collections::BTreeSet;

use interaction::HoverResult;

use crate::{
    ClickSequence, Hovered, Selectable, Selected, SelectionOperation, SelectionState,
};

pub fn selection_filter_shortcut_system(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,

    mut filter: ResMut<SelectionFilter>,
    mut hover: ResMut<HoverResult>,
    mut selection: ResMut<SelectionState>,
    mut click_sequence: ResMut<ClickSequence>,

    hovered_query: Query<Entity, With<Hovered>>,
    selected_query: Query<Entity, With<Selected>>,
    viewport_tool: Res<ViewportTool>,
    keyboard_state: Res<UiKeyboardState>,
) {
    if *viewport_tool != ViewportTool::Selection || keyboard_state.text_editing {
        return;
    }

    let requested_level = if keyboard.just_pressed(KeyCode::Digit1) {
        Some(SelectionLevel::Node)
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        Some(SelectionLevel::Edge)
    } else if keyboard.just_pressed(KeyCode::Digit3) {
        Some(SelectionLevel::Face)
    } else if keyboard.just_pressed(KeyCode::Digit4) {
        Some(SelectionLevel::Element)
    } else {
        None
    };

    let Some(level) = requested_level else {
        return;
    };

    if filter.level == level {
        return;
    }

    filter.level = level;
    hover.clear();
    selection.clear();
    click_sequence.reset();

    for entity in hovered_query.iter() {
        commands.entity(entity).remove::<Hovered>();
    }

    for entity in selected_query.iter() {
        commands.entity(entity).remove::<Selected>();
    }
}

pub fn click_selection_system(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,

    hover: Res<HoverResult>,
    hover_preview: Res<fem_core::HoverPreviewTargets>,
    filter: Res<SelectionFilter>,
    ui_pointer: Res<UiPointerState>,
    model: Option<Res<FemModel>>,
    viewport_tool: Res<ViewportTool>,

    mut selection: ResMut<SelectionState>,
    mut click_sequence: ResMut<ClickSequence>,

    selected_query: Query<Entity, With<Selected>>,
    selectable_query: Query<&Selectable>,
) {
    if *viewport_tool != ViewportTool::Selection {
        click_sequence.reset();
        return;
    }

    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if ui_pointer.over_ui {
        click_sequence.reset();
        return;
    }

    let ctrl  = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let alt   = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);

    let operation = SelectionOperation::from_modifiers(ctrl, shift, alt);

    if operation == SelectionOperation::Replace {
        for entity in selected_query.iter() {
            commands.entity(entity).remove::<Selected>();
        }
        selection.clear();
    }

    let Some(hit) = hover.hit else {
        click_sequence.reset();
        return;
    };
    let hover_target = hit.target;

    let level = hover_target.level();

    if !filter.accepts(level) || hover.level() != Some(level) {
        click_sequence.reset();
        return;
    }

    let click_count = click_sequence.register(hover_target, time.elapsed_secs_f64());

    // What gets committed is whichever group `fem_core::HoverPreviewTargets`
    // is currently showing for this hover — a live Coplanar/Smooth expansion
    // or just the single hovered target (also
    // covering Node/Edge selection, which the preview never expands). This
    // way a click always applies the chosen selection operation to exactly what
    // was highlighted a moment before clicking, whether that's one facet
    // or an entire curved surface.
    let fallback_targets = if hover_preview.targets.is_empty() {
        vec![hover_target]
    } else {
        hover_preview.targets.clone()
    };
    let fallback_highlights = if hover_preview.highlight_targets.is_empty() {
        fallback_targets.clone()
    } else {
        hover_preview.highlight_targets.clone()
    };
    let group = selection_group_for_click(
        click_count,
        hit,
        model.as_deref(),
        fallback_targets,
        fallback_highlights,
    );

    let removes = selection.will_remove_group(&group.targets, operation);
    selection.apply_group(&group.targets, &group.highlights, operation);

    // `selection.entities` / the `Selected` ECS marker only ever track the
    // one concrete pickable entity directly under the cursor — the rest of
    // a grown surface group is picked by id (already pushed into
    // `selection.targets` above), not by its own `Selectable` entity, so
    // there's nothing further to mark here.
    if let Some(entity) = hover.entity {
        if let Ok(selectable) = selectable_query.get(entity) {
            if selectable.level() == level {
                if removes {
                    commands.entity(entity).remove::<Selected>();
                    selection.entities.retain(|&selected| selected != entity);
                } else if !selection.entities.contains(&entity) {
                    commands.entity(entity).insert(Selected);
                    selection.entities.push(entity);
                }
            }
        }
    }
}

pub fn clear_selection_shortcut_system(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut selection: ResMut<SelectionState>,
    mut click_sequence: ResMut<ClickSequence>,
    selected_query: Query<Entity, With<Selected>>,
    keyboard_state: Res<UiKeyboardState>,
) {
    if keyboard_state.text_editing || !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }

    for entity in &selected_query {
        commands.entity(entity).remove::<Selected>();
    }
    selection.clear();
    click_sequence.reset();
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectionGroup {
    targets: Vec<FemEntityRef>,
    highlights: Vec<FemEntityRef>,
}

impl SelectionGroup {
    fn new(targets: Vec<FemEntityRef>, highlights: Vec<FemEntityRef>) -> Self {
        let targets = sorted_unique(targets);
        let mut highlights = sorted_unique(highlights);
        if highlights.is_empty() {
            highlights = targets.clone();
        }
        Self {
            targets,
            highlights,
        }
    }
}

fn selection_group_for_click(
    click_count: u8,
    hit: SelectionHit,
    model: Option<&FemModel>,
    fallback_targets: Vec<FemEntityRef>,
    fallback_highlights: Vec<FemEntityRef>,
) -> SelectionGroup {
    let expanded = match click_count {
        2 => model.and_then(|model| connected_boundary_group(model, hit)),
        3 => model.and_then(|model| connected_component_group(model, hit)),
        _ => None,
    };

    expanded.unwrap_or_else(|| SelectionGroup::new(fallback_targets, fallback_highlights))
}

/// Double-click keeps the active topology level but expands along the
/// boundary without applying a face-normal angle threshold.
fn connected_boundary_group(model: &FemModel, hit: SelectionHit) -> Option<SelectionGroup> {
    let mesh_index = hit.target.mesh_index;
    let mesh = model.meshes.get(mesh_index)?;

    match hit.target.entity {
        FemEntityId::Node(seed_node) => {
            let seed_edge = mesh
                .cached_boundary_edges()
                .iter()
                .find(|edge| edge.nodes.contains(&seed_node))?
                .id;
            let edge_ids = expand_connected_boundary_edges(mesh, seed_edge);
            let nodes: BTreeSet<NodeId> = edge_ids
                .iter()
                .filter_map(|id| {
                    mesh.cached_boundary_edges()
                        .iter()
                        .find(|edge| edge.id == *id)
                })
                .flat_map(|edge| edge.nodes)
                .collect();
            let targets = nodes
                .into_iter()
                .map(|node| FemEntityRef::node(mesh_index, node))
                .collect();
            Some(SelectionGroup::new(targets, Vec::new()))
        }
        FemEntityId::Edge(seed_edge) => {
            let targets = expand_connected_feature_edges(
                mesh,
                seed_edge,
                DEFAULT_FEATURE_EDGE_ANGLE_DEG,
            )
                .into_iter()
                .map(|edge| FemEntityRef::edge(mesh_index, edge))
                .collect();
            Some(SelectionGroup::new(targets, Vec::new()))
        }
        FemEntityId::Face(seed_face) => {
            let seed_face = hit.surface_face.unwrap_or(seed_face);
            let targets = expand_connected_boundary_faces(mesh, seed_face)
                .into_iter()
                .map(|face| FemEntityRef::face(mesh_index, face))
                .collect();
            Some(SelectionGroup::new(targets, Vec::new()))
        }
        FemEntityId::Element(seed_element) => {
            let seed_face = hit.surface_face.or_else(|| {
                mesh.cached_boundary_faces()
                    .iter()
                    .find(|face| face.element == Some(seed_element))
                    .map(|face| face.id)
            })?;
            let face_ids = expand_connected_boundary_faces(mesh, seed_face);
            let face_set: BTreeSet<_> = face_ids.iter().copied().collect();
            let targets = mesh
                .cached_boundary_faces()
                .iter()
                .filter(|face| face_set.contains(&face.id))
                .filter_map(|face| face.element)
                .map(|element| FemEntityRef::element(mesh_index, element))
                .collect();
            let highlights = face_ids
                .into_iter()
                .map(|face| FemEntityRef::face(mesh_index, face))
                .collect();
            Some(SelectionGroup::new(targets, highlights))
        }
    }
}

/// Triple-click expands through element connectivity. The semantic targets
/// still match the active filter, while Element highlighting uses only the
/// component's exterior faces so internal tetrahedral faces never leak into
/// the overlay.
fn connected_component_group(model: &FemModel, hit: SelectionHit) -> Option<SelectionGroup> {
    let mesh_index = hit.target.mesh_index;
    let mesh = model.meshes.get(mesh_index)?;
    let seed_element = hit.element.or_else(|| element_behind_target(mesh, hit.target.entity))?;
    let element_ids = expand_connected_elements(mesh, seed_element);
    let element_set: BTreeSet<_> = element_ids.iter().copied().collect();

    match hit.target.entity {
        FemEntityId::Element(_) => {
            let targets = element_ids
                .into_iter()
                .map(|element| FemEntityRef::element(mesh_index, element))
                .collect();
            let highlights = mesh
                .cached_boundary_faces()
                .iter()
                .filter(|face| face.element.is_some_and(|id| element_set.contains(&id)))
                .map(|face| FemEntityRef::face(mesh_index, face.id))
                .collect();
            Some(SelectionGroup::new(targets, highlights))
        }
        FemEntityId::Face(_) => {
            let targets = mesh
                .cached_boundary_faces()
                .iter()
                .filter(|face| face.element.is_some_and(|id| element_set.contains(&id)))
                .map(|face| FemEntityRef::face(mesh_index, face.id))
                .collect();
            Some(SelectionGroup::new(targets, Vec::new()))
        }
        FemEntityId::Edge(_) => {
            let component_edges: BTreeSet<_> = mesh
                .elements
                .iter()
                .filter(|element| element_set.contains(&element.id))
                .flat_map(|element| element.edge_node_ids())
                .map(|[start, end]| ordered_pair(start, end))
                .collect();
            let targets = mesh
                .cached_boundary_edges()
                .iter()
                .filter(|edge| component_edges.contains(&ordered_pair(edge.nodes[0], edge.nodes[1])))
                .map(|edge| FemEntityRef::edge(mesh_index, edge.id))
                .collect();
            Some(SelectionGroup::new(targets, Vec::new()))
        }
        FemEntityId::Node(_) => {
            let targets = mesh
                .elements
                .iter()
                .filter(|element| element_set.contains(&element.id))
                .flat_map(|element| element.nodes.iter().copied())
                .map(|node| FemEntityRef::node(mesh_index, node))
                .collect();
            Some(SelectionGroup::new(targets, Vec::new()))
        }
    }
}

fn element_behind_target(mesh: &FemMesh, target: FemEntityId) -> Option<ElementId> {
    match target {
        FemEntityId::Element(element) => Some(element),
        FemEntityId::Face(face_id) => mesh
            .cached_boundary_faces()
            .iter()
            .find(|face| face.id == face_id)
            .and_then(|face| face.element),
        FemEntityId::Edge(edge_id) => {
            let edge = mesh.cached_edges().iter().find(|edge| edge.id == edge_id)?;
            let pair = ordered_pair(edge.nodes[0], edge.nodes[1]);
            mesh.elements
                .iter()
                .find(|element| {
                    element
                        .edge_node_ids()
                        .iter()
                        .any(|[start, end]| ordered_pair(*start, *end) == pair)
                })
                .map(|element| element.id)
        }
        FemEntityId::Node(node) => mesh
            .elements
            .iter()
            .find(|element| element.nodes.contains(&node))
            .map(|element| element.id),
    }
}

fn ordered_pair(start: NodeId, end: NodeId) -> (NodeId, NodeId) {
    if start <= end {
        (start, end)
    } else {
        (end, start)
    }
}

fn sorted_unique(targets: Vec<FemEntityRef>) -> Vec<FemEntityRef> {
    targets.into_iter().collect::<BTreeSet<_>>().into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_double_click_keeps_element_targets_and_surface_highlights() {
        let mesh = FemMesh::demo_hex8();
        let face = &mesh.cached_boundary_faces()[0];
        let element = face.element.expect("demo face has an owner");
        let hit = SelectionHit::new(FemEntityRef::element(0, element), Vec3::ZERO, 1.0)
            .with_surface(face.id, Some(element));
        let model = FemModel::single_mesh("demo", mesh);

        let group = connected_boundary_group(&model, hit).expect("boundary group");

        assert_eq!(group.targets, vec![FemEntityRef::element(0, element)]);
        assert_eq!(group.highlights.len(), 6);
        assert!(
            group
                .highlights
                .iter()
                .all(|target| matches!(target.entity, FemEntityId::Face(_)))
        );
    }

    #[test]
    fn edge_double_click_produces_only_edge_targets() {
        let mesh = FemMesh::demo_hex8();
        let edge = mesh.cached_boundary_edges()[0].id;
        let hit = SelectionHit::new(FemEntityRef::edge(0, edge), Vec3::ZERO, 1.0);
        let model = FemModel::single_mesh("demo", mesh);

        let group = connected_boundary_group(&model, hit).expect("edge boundary group");

        assert_eq!(group.targets.len(), 12);
        assert_eq!(group.highlights, group.targets);
        assert!(
            group
                .targets
                .iter()
                .all(|target| matches!(target.entity, FemEntityId::Edge(_)))
        );
    }

    #[test]
    fn element_triple_click_highlights_only_boundary_faces() {
        let mesh = FemMesh::demo_hex8();
        let face = &mesh.cached_boundary_faces()[0];
        let element = face.element.expect("demo face has an owner");
        let hit = SelectionHit::new(FemEntityRef::element(0, element), Vec3::ZERO, 1.0)
            .with_surface(face.id, Some(element));
        let model = FemModel::single_mesh("demo", mesh);

        let group = connected_component_group(&model, hit).expect("component group");

        assert_eq!(group.targets, vec![FemEntityRef::element(0, element)]);
        assert_eq!(group.highlights.len(), 6);
        assert!(
            group
                .highlights
                .iter()
                .all(|target| matches!(target.entity, FemEntityId::Face(_)))
        );
    }
}
