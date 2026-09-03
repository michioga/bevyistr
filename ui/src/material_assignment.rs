//! Viewport-first object picking for the Materials workflow. FEM topology
//! selection is deliberately inactive here; named groups remain in the list.
use crate::layout::SidebarPage;
use crate::material_library::MaterialLibraryState;
use crate::materials_ui::{
    AssignmentTarget, SelectedEgrp, SelectedMaterialForSection, choose_assignment_target,
};
use bevy::prelude::*;
use fem_core::{
    FemModel, FemModelVersion, HoverPreviewTargets, MainViewportCamera, UiKeyboardState,
    UiPointerState, ViewportTool,
};
use interaction::HoverResult;
use selection::{Hovered, Selected, SelectionState};

#[derive(Resource, Default)]
pub(crate) struct MaterialViewportHover(pub Option<usize>);

pub(crate) fn material_assignment_tool(
    mut commands: Commands,
    page: Res<SidebarPage>,
    mut tool: ResMut<ViewportTool>,
    mut selection: ResMut<SelectionState>,
    mut hover: ResMut<HoverResult>,
    mut preview: ResMut<HoverPreviewTargets>,
    mut part_hover: ResMut<MaterialViewportHover>,
    marked: Query<Entity, Or<(With<Selected>, With<Hovered>)>>,
) {
    if *page == SidebarPage::Materials && *tool != ViewportTool::MaterialAssignment {
        *tool = ViewportTool::MaterialAssignment;
        selection.clear();
        hover.clear();
        preview.targets.clear();
        preview.highlight_targets.clear();
        for entity in &marked {
            commands.entity(entity).remove::<(Selected, Hovered)>();
        }
    } else if *page != SidebarPage::Materials {
        part_hover.0 = None;
        if *tool == ViewportTool::MaterialAssignment {
            *tool = ViewportTool::Selection;
        }
    }
}

pub(crate) fn material_assignment_hover(
    tool: Res<ViewportTool>,
    pointer: Res<UiPointerState>,
    model: Option<Res<FemModel>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainViewportCamera>>,
    mut hover: ResMut<MaterialViewportHover>,
) {
    let hit = || -> Option<usize> {
        if *tool != ViewportTool::MaterialAssignment || pointer.over_ui {
            return None;
        }
        let cursor = windows.single().ok()?.cursor_position()?;
        let (camera, transform) = cameras.single().ok()?;
        let ray = camera.viewport_to_world(transform, cursor).ok()?;
        Some(
            picking::pick_part(model.as_deref()?, ray.origin, *ray.direction)?
                .target
                .mesh_index,
        )
    };
    let next = hit();
    if hover.0 != next {
        hover.0 = next;
    }
}

pub(crate) fn material_assignment_click(
    tool: Res<ViewportTool>,
    page: Res<SidebarPage>,
    pointer: Res<UiPointerState>,
    keyboard: Res<UiKeyboardState>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    hover: Res<MaterialViewportHover>,
    mut target: ResMut<SelectedEgrp>,
    mut material: ResMut<SelectedMaterialForSection>,
    mut library: ResMut<MaterialLibraryState>,
) {
    if *page != SidebarPage::Materials || *tool != ViewportTool::MaterialAssignment {
        return;
    }
    if !keyboard.text_editing && keys.just_pressed(KeyCode::Escape) {
        choose_assignment_target(&mut target, None, &mut material, &mut library);
    } else if !pointer.over_ui && buttons.just_pressed(MouseButton::Left) {
        let next = hover.0.map(|mesh_index| AssignmentTarget {
            mesh_index,
            group: None,
        });
        choose_assignment_target(&mut target, next, &mut material, &mut library);
    }
}

fn target_bounds(model: &FemModel, target: &AssignmentTarget) -> Option<(Vec3, Vec3)> {
    let mesh = model.meshes.get(target.mesh_index)?;
    let Some(group) = &target.group else {
        return mesh.bounds();
    };
    let group = mesh.element_sets.iter().find(|g| &g.name == group)?;
    let ids: std::collections::BTreeSet<_> = group.elements.iter().copied().collect();
    let nodes: std::collections::BTreeSet<_> = mesh
        .elements
        .iter()
        .filter(|e| ids.contains(&e.id))
        .flat_map(|e| e.nodes.iter().copied())
        .collect();
    let mut points = mesh
        .nodes
        .iter()
        .filter(|n| nodes.contains(&n.id))
        .map(|n| n.position);
    let first = points.next()?;
    Some(points.fold((first, first), |(min, max), p| (min.min(p), max.max(p))))
}

#[derive(Default)]
pub(crate) struct BoundsCache {
    key: Option<(u64, AssignmentTarget)>,
    bounds: Option<(Vec3, Vec3)>,
}
impl BoundsCache {
    fn get(
        &mut self,
        model: &FemModel,
        version: u64,
        target: &AssignmentTarget,
    ) -> Option<(Vec3, Vec3)> {
        if self.key.as_ref() != Some(&(version, target.clone())) {
            self.bounds = target_bounds(model, target);
            self.key = Some((version, target.clone()));
        }
        self.bounds
    }
}
pub(crate) fn draw_material_target(
    page: Res<SidebarPage>,
    model: Option<Res<FemModel>>,
    version: Res<FemModelVersion>,
    target: Res<SelectedEgrp>,
    hover: Res<MaterialViewportHover>,
    mut selected_cache: Local<BoundsCache>,
    mut hover_cache: Local<BoundsCache>,
    mut gizmos: Gizmos,
) {
    if *page != SidebarPage::Materials {
        return;
    }
    let Some(model) = model.as_deref() else {
        return;
    };
    if let Some(target) = &target.0 {
        if let Some(bounds) = selected_cache.get(model, version.value, target) {
            outline(&mut gizmos, bounds, Color::srgb(0.1, 0.85, 1.0));
        }
    }
    if let Some(mesh_index) = hover
        .0
        .filter(|i| target.0.as_ref().is_none_or(|t| t.mesh_index != *i))
    {
        let target = AssignmentTarget {
            mesh_index,
            group: None,
        };
        if let Some(bounds) = hover_cache.get(model, version.value, &target) {
            outline(&mut gizmos, bounds, Color::srgb(1.0, 0.8, 0.15));
        }
    }
}
fn outline(gizmos: &mut Gizmos, (min, max): (Vec3, Vec3), color: Color) {
    let p = [
        min,
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        max,
        Vec3::new(min.x, max.y, max.z),
    ];
    for (a, b) in [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ] {
        gizmos.line(p[a], p[b], color);
    }
}
