use bevy::prelude::*;
use fem_core::{SelectionFilter, SelectionLevel, UiPointerState};

use interaction::HoverResult;

use crate::{Hovered, Selectable, Selected, SelectionOperation, SelectionState};

pub fn selection_filter_shortcut_system(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,

    mut filter: ResMut<SelectionFilter>,
    mut hover: ResMut<HoverResult>,
    mut selection: ResMut<SelectionState>,

    hovered_query: Query<Entity, With<Hovered>>,
    selected_query: Query<Entity, With<Selected>>,
) {
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

    hover: Res<HoverResult>,
    hover_preview: Res<fem_core::HoverPreviewTargets>,
    filter: Res<SelectionFilter>,
    ui_pointer: Res<UiPointerState>,

    mut selection: ResMut<SelectionState>,

    selected_query: Query<Entity, With<Selected>>,
    selectable_query: Query<&Selectable>,
) {
    if !buttons.just_pressed(MouseButton::Left) || ui_pointer.over_ui {
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

    let Some(hover_target) = hover.target() else { return; };

    let level = hover_target.level();

    if !filter.accepts(level) || hover.level() != Some(level) {
        return;
    }

    // What gets committed is whichever group `fem_core::HoverPreviewTargets`
    // is currently showing for this hover — a live Coplanar/Smooth expansion
    // or just the single hovered target (also
    // covering Node/Edge selection, which the preview never expands). This
    // way a click always applies the chosen selection operation to exactly what
    // was highlighted a moment before clicking, whether that's one facet
    // or an entire curved surface.
    let group: &[fem_core::FemEntityRef] = if hover_preview.targets.is_empty() {
        std::slice::from_ref(&hover_target)
    } else {
        &hover_preview.targets
    };
    let highlight_group: &[fem_core::FemEntityRef] = if hover_preview.highlight_targets.is_empty() {
        group
    } else {
        &hover_preview.highlight_targets
    };

    let removes = selection.will_remove_group(group, operation);
    selection.apply_group(group, highlight_group, operation);

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
    selected_query: Query<Entity, With<Selected>>,
) {
    if !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }

    for entity in &selected_query {
        commands.entity(entity).remove::<Selected>();
    }
    selection.clear();
}
