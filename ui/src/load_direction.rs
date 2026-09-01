//! Viewport-first global direction picker for nodal loads.
//!
//! The ordinary axis buttons remain available for deterministic keyboard-like
//! input. This tool adds the complementary CAD workflow: keep the selected
//! nodes visible, click "Pick Direction in Viewport", then click one of the
//! six X-ray arrows at the selection centroid. Magnitude still belongs to the
//! shared numeric measurement box and is committed only by Apply Load.

use crate::assembly::VIEWPORT_GIZMO_RENDER_LAYER;
use crate::bc_loads_ui::{ActiveLoadEditor, SelectedLoadDirection};
use crate::boundary_editor::BoundaryLoadEditorState;
use crate::measurement::MeasurementBoxState;
use crate::slider::{SliderId, SliderState, SliderTrack};
use bevy::camera::visibility::RenderLayers;
use bevy::math::primitives::{Cone, Cuboid, Cylinder};
use bevy::mesh::Mesh3d;
use bevy::pbr::MeshMaterial3d;
use bevy::prelude::*;
use fem_core::{
    FemEntityId, FemModel, MainViewportCamera, UiKeyboardState, UiPointerState, ViewportTool,
};
use selection::SelectionState;

const PICK_RADIUS_PX: f32 = 13.0;
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);
const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const ACTIVE_BORDER: Color = Color::srgb(0.57, 0.86, 0.92);

#[derive(Component)]
pub(crate) struct LoadDirectionPickerButton;

#[derive(Component)]
pub(crate) struct LoadDirectionPickerLabel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DirectionChoice {
    dof: u8,
    sign: i8,
}

impl DirectionChoice {
    const fn new(dof: u8, sign: i8) -> Self {
        Self { dof, sign }
    }

    fn vector(self) -> Vec3 {
        let axis = match self.dof {
            1 => Vec3::X,
            2 => Vec3::Y,
            3 => Vec3::Z,
            _ => Vec3::ZERO,
        };
        axis * self.sign as f32
    }

    const fn selected_value(self) -> (u8, f32) {
        (self.dof, self.sign as f32)
    }

    const fn label(self) -> &'static str {
        match (self.dof, self.sign) {
            (1, 1) => "+X",
            (1, -1) => "-X",
            (2, 1) => "+Y",
            (2, -1) => "-Y",
            (3, 1) => "+Z",
            (3, -1) => "-Z",
            _ => "?",
        }
    }
}

const DIRECTIONS: [DirectionChoice; 6] = [
    DirectionChoice::new(1, 1),
    DirectionChoice::new(1, -1),
    DirectionChoice::new(2, 1),
    DirectionChoice::new(2, -1),
    DirectionChoice::new(3, 1),
    DirectionChoice::new(3, -1),
];

#[derive(Resource, Debug, Default)]
pub(crate) struct LoadDirectionPickerState {
    hovered: Option<DirectionChoice>,
    finish_pending: bool,
}

#[derive(Component)]
pub(crate) struct LoadDirectionGizmoPiece {
    choice: Option<DirectionChoice>,
    offset: f32,
    rotation: Quat,
    normal_material: Handle<StandardMaterial>,
    hover_material: Handle<StandardMaterial>,
    selected_material: Handle<StandardMaterial>,
}

#[derive(Debug, Clone, Copy)]
struct PickerAnchor {
    center: Vec3,
    size: f32,
}

pub(crate) fn spawn_load_direction_gizmo(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let hover_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.78, 0.16, 0.94),
        emissive: LinearRgba::rgb(0.80, 0.44, 0.05),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let selected_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.90, 0.97, 1.0, 0.94),
        emissive: LinearRgba::rgb(0.28, 0.55, 0.72),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let origin_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.82, 0.88, 0.92, 0.90),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.11, 0.11, 0.11))),
        MeshMaterial3d(origin_material.clone()),
        RenderLayers::layer(VIEWPORT_GIZMO_RENDER_LAYER),
        Transform::default(),
        Visibility::Hidden,
        LoadDirectionGizmoPiece {
            choice: None,
            offset: 0.0,
            rotation: Quat::IDENTITY,
            normal_material: origin_material.clone(),
            hover_material: origin_material.clone(),
            selected_material: origin_material,
        },
        Name::new("Load direction compass origin"),
    ));

    for choice in DIRECTIONS {
        let color = direction_color(choice);
        let normal_material = materials.add(StandardMaterial {
            base_color: color,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        });
        let rotation = Quat::from_rotation_arc(Vec3::Y, choice.vector());

        commands.spawn((
            Mesh3d(meshes.add(Cylinder {
                radius: 0.034,
                half_height: 0.25,
            })),
            MeshMaterial3d(normal_material.clone()),
            RenderLayers::layer(VIEWPORT_GIZMO_RENDER_LAYER),
            Transform::default(),
            Visibility::Hidden,
            LoadDirectionGizmoPiece {
                choice: Some(choice),
                offset: 0.30,
                rotation,
                normal_material: normal_material.clone(),
                hover_material: hover_material.clone(),
                selected_material: selected_material.clone(),
            },
            Name::new(format!("Load direction {} shaft", choice.label())),
        ));

        commands.spawn((
            Mesh3d(meshes.add(Cone {
                radius: 0.105,
                height: 0.22,
            })),
            MeshMaterial3d(normal_material.clone()),
            RenderLayers::layer(VIEWPORT_GIZMO_RENDER_LAYER),
            Transform::default(),
            Visibility::Hidden,
            LoadDirectionGizmoPiece {
                choice: Some(choice),
                offset: 0.66,
                rotation,
                normal_material,
                hover_material: hover_material.clone(),
                selected_material: selected_material.clone(),
            },
            Name::new(format!("Load direction {} head", choice.label())),
        ));
    }
}

pub(crate) fn load_direction_picker_button_system(
    mut tool: ResMut<ViewportTool>,
    selection: Res<SelectionState>,
    mut state: ResMut<LoadDirectionPickerState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<LoadDirectionPickerButton>,
    >,
    mut labels: Query<&mut Text, With<LoadDirectionPickerLabel>>,
) {
    let node_count = selected_node_count(&selection);
    let Ok((interaction, mut background, mut border)) = buttons.single_mut() else {
        return;
    };

    if *interaction == Interaction::Pressed && interaction.is_changed() {
        if *tool == ViewportTool::LoadDirection {
            *tool = ViewportTool::Selection;
            state.hovered = None;
            state.finish_pending = false;
        } else if node_count > 0 {
            *tool = ViewportTool::LoadDirection;
            state.hovered = None;
            state.finish_pending = false;
        }
    }

    let active = *tool == ViewportTool::LoadDirection;
    *background = BackgroundColor(match (*interaction, active) {
        (Interaction::Pressed, _) => BUTTON_PRESSED,
        (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
        (Interaction::Hovered, false) => BUTTON_HOVERED,
        (Interaction::None, false) => BUTTON_NORMAL,
    });
    *border = BorderColor::all(if active { ACTIVE_BORDER } else { PANEL_BORDER });

    if let Ok(mut label) = labels.single_mut() {
        **label = if active {
            "Direction picker: click an arrow".to_string()
        } else if node_count == 0 {
            "Pick direction in viewport - select nodes first".to_string()
        } else {
            format!("Pick direction in viewport ({node_count} nodes)")
        };
    }
}

pub(crate) fn load_direction_picker_hover_system(
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainViewportCamera>>,
    model: Option<Res<FemModel>>,
    selection: Res<SelectionState>,
    tool: Res<ViewportTool>,
    ui_pointer: Res<UiPointerState>,
    mut state: ResMut<LoadDirectionPickerState>,
) {
    if *tool != ViewportTool::LoadDirection || ui_pointer.over_ui {
        state.hovered = None;
        return;
    }
    let Some(anchor) = model
        .as_deref()
        .and_then(|model| picker_anchor(&selection, model))
    else {
        state.hovered = None;
        return;
    };
    let Some(cursor) = windows.single().ok().and_then(Window::cursor_position) else {
        state.hovered = None;
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        state.hovered = None;
        return;
    };
    state.hovered = pick_direction(camera, camera_transform, cursor, anchor);
}

pub(crate) fn load_direction_picker_input_system(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    keyboard_state: Res<UiKeyboardState>,
    ui_pointer: Res<UiPointerState>,
    mut tool: ResMut<ViewportTool>,
    mut state: ResMut<LoadDirectionPickerState>,
    mut selected: ResMut<SelectedLoadDirection>,
    mut active_editor: ResMut<ActiveLoadEditor>,
    mut editor: ResMut<BoundaryLoadEditorState>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut measurement: ResMut<MeasurementBoxState>,
) {
    if state.finish_pending && !buttons.just_pressed(MouseButton::Left) {
        state.finish_pending = false;
        state.hovered = None;
        if *tool == ViewportTool::LoadDirection {
            *tool = ViewportTool::Selection;
        }
        return;
    }
    if *tool != ViewportTool::LoadDirection {
        state.hovered = None;
        state.finish_pending = false;
        return;
    }
    if !keyboard_state.text_editing && keyboard.just_pressed(KeyCode::Escape) {
        *tool = ViewportTool::Selection;
        state.hovered = None;
        return;
    }
    if ui_pointer.over_ui || !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(choice) = state.hovered else {
        return;
    };

    selected.0 = Some(choice.selected_value());
    *active_editor = ActiveLoadEditor::Nodal;
    let magnitude = sliders
        .iter()
        .find(|slider| slider.id == SliderId::LoadMagnitude)
        .map(|slider| slider.value)
        .unwrap_or(100.0);
    editor.set_axis_force(choice.dof, choice.sign as f32, magnitude);
    measurement.begin_slider_value(
        SliderId::LoadMagnitude,
        measurement_label(choice),
        "analysis force units",
        magnitude,
    );
    state.finish_pending = true;
}

pub(crate) fn update_load_direction_gizmo_visuals(
    model: Option<Res<FemModel>>,
    selection: Res<SelectionState>,
    tool: Res<ViewportTool>,
    state: Res<LoadDirectionPickerState>,
    selected: Res<SelectedLoadDirection>,
    mut pieces: Query<(
        &LoadDirectionGizmoPiece,
        &mut Transform,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let anchor = (*tool == ViewportTool::LoadDirection)
        .then(|| {
            model
                .as_deref()
                .and_then(|model| picker_anchor(&selection, model))
        })
        .flatten();
    let Some(anchor) = anchor else {
        for (_, _, mut visibility, _) in &mut pieces {
            *visibility = Visibility::Hidden;
        }
        return;
    };

    for (piece, mut transform, mut visibility, mut material) in &mut pieces {
        let direction = piece
            .choice
            .map(DirectionChoice::vector)
            .unwrap_or(Vec3::ZERO);
        transform.translation = anchor.center + direction * piece.offset * anchor.size;
        transform.rotation = piece.rotation;
        transform.scale = Vec3::splat(anchor.size);
        material.0 = if piece.choice.is_some() && piece.choice == state.hovered {
            piece.hover_material.clone()
        } else if piece
            .choice
            .is_some_and(|choice| selected.0 == Some(choice.selected_value()))
        {
            piece.selected_material.clone()
        } else {
            piece.normal_material.clone()
        };
        *visibility = Visibility::Visible;
    }
}

fn selected_node_count(selection: &SelectionState) -> usize {
    selection
        .targets
        .iter()
        .filter(|target| matches!(target.entity, FemEntityId::Node(_)))
        .count()
}

fn picker_anchor(selection: &SelectionState, model: &FemModel) -> Option<PickerAnchor> {
    let positions: Vec<Vec3> = selection
        .targets
        .iter()
        .filter_map(|target| {
            let FemEntityId::Node(node) = target.entity else {
                return None;
            };
            model.meshes.get(target.mesh_index)?.node_position(node)
        })
        .collect();
    if positions.is_empty() {
        return None;
    }

    let center = positions.iter().copied().sum::<Vec3>() / positions.len() as f32;
    let (minimum, maximum) = positions.iter().copied().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(minimum, maximum), point| (minimum.min(point), maximum.max(point)),
    );
    let model_size = model
        .bounds()
        .map(|(minimum, maximum)| minimum.distance(maximum))
        .filter(|size| size.is_finite() && *size > 1.0e-8)
        .unwrap_or(1.0);
    let selected_size = minimum.distance(maximum);
    let preferred = if selected_size > model_size * 1.0e-6 {
        selected_size * 0.32
    } else {
        model_size * 0.08
    };
    let size = preferred.clamp(model_size * 0.035, model_size * 0.16);
    Some(PickerAnchor { center, size })
}

fn pick_direction(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    cursor: Vec2,
    anchor: PickerAnchor,
) -> Option<DirectionChoice> {
    DIRECTIONS
        .into_iter()
        .filter_map(|choice| {
            let direction = choice.vector();
            let start = camera
                .world_to_viewport(
                    camera_transform,
                    anchor.center + direction * anchor.size * 0.10,
                )
                .ok()?;
            let end = camera
                .world_to_viewport(
                    camera_transform,
                    anchor.center + direction * anchor.size * 0.84,
                )
                .ok()?;
            if start.distance_squared(end) < 36.0 {
                return None;
            }
            let distance = point_segment_distance(cursor, start, end);
            (distance <= PICK_RADIUS_PX).then_some((choice, distance))
        })
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(choice, _)| choice)
}

fn point_segment_distance(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_squared();
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let amount = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * amount)
}

fn direction_color(choice: DirectionChoice) -> Color {
    match (choice.dof, choice.sign) {
        (1, 1) => Color::srgba(0.92, 0.16, 0.18, 0.88),
        (1, -1) => Color::srgba(0.52, 0.09, 0.11, 0.88),
        (2, 1) => Color::srgba(0.20, 0.82, 0.30, 0.88),
        (2, -1) => Color::srgba(0.10, 0.48, 0.17, 0.88),
        (3, 1) => Color::srgba(0.18, 0.42, 1.0, 0.88),
        (3, -1) => Color::srgba(0.09, 0.23, 0.58, 0.88),
        _ => Color::WHITE,
    }
}

fn measurement_label(choice: DirectionChoice) -> &'static str {
    match (choice.dof, choice.sign) {
        (1, 1) => "Nodal load +X",
        (1, -1) => "Nodal load -X",
        (2, 1) => "Nodal load +Y",
        (2, -1) => "Nodal load -Y",
        (3, 1) => "Nodal load +Z",
        (3, -1) => "Nodal load -Z",
        _ => "Nodal load",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fem_core::{FemEntityRef, FemMesh, FemModel, FemNode, NodeId};

    #[test]
    fn direction_choices_map_to_frontistr_dofs_and_signs() {
        assert_eq!(DirectionChoice::new(1, -1).vector(), Vec3::NEG_X);
        assert_eq!(DirectionChoice::new(2, 1).selected_value(), (2, 1.0));
        assert_eq!(DirectionChoice::new(3, -1).label(), "-Z");
    }

    #[test]
    fn picker_anchor_uses_selected_node_centroid() {
        let mesh = FemMesh::new(
            vec![
                FemNode::new(NodeId(1), Vec3::new(-2.0, 0.0, 0.0)),
                FemNode::new(NodeId(2), Vec3::new(2.0, 0.0, 0.0)),
            ],
            Vec::new(),
        );
        let model = FemModel::single_mesh("mesh", mesh);
        let mut selection = SelectionState::default();
        selection.targets = vec![
            FemEntityRef::node(0, NodeId(1)),
            FemEntityRef::node(0, NodeId(2)),
        ];

        let anchor = picker_anchor(&selection, &model).expect("selected-node anchor");
        assert!(anchor.center.length() < 1.0e-6);
        assert!(anchor.size > 0.0);
    }

    #[test]
    fn screen_distance_hits_the_middle_of_an_arrow() {
        let distance =
            point_segment_distance(Vec2::new(5.0, 1.0), Vec2::ZERO, Vec2::new(10.0, 0.0));
        assert!((distance - 1.0).abs() < 1.0e-6);
    }
}
