use bevy::prelude::*;
use fem_core::{SelectionFilter, SelectionLevel, UiPointerState};

use interaction::HoverResult;

use crate::{Hovered, Selectable, Selected, SelectionState};

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

    // Ctrl and Shift both mean "add to the current selection instead of
    // replacing it" — kept as two separate keys (rather than just Ctrl)
    // since Shift is the more natural "add another group" modifier once a
    // click can commit a whole grown surface at once, not just one facet.
    // Alt means "remove instead" — useful because a smooth surface walk can
    // judge a surface boundary from facet-to-facet normal angle, so on a
    // smoothly-faceted fillet or chamfer (each facet only a degree or two
    // off its neighbour) it can walk right past the edge a person can see
    // by eye onto an adjacent surface that just happens to curve gently
    // enough to stay under the threshold the whole way. There's no purely
    // local angle check that can always tell "still curving around the
    // same surface" apart from "smoothly blending onto a different one" —
    // so rather than chase a threshold that may not exist for a given
    // mesh, Alt+click lets a person manually peel the last few
    // over-included faces/elements back off after an automatic expand.
    let accumulate = ctrl || shift || alt;

    if !accumulate {
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
    // way a click always selects (or, with Alt, deselects) exactly what
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

    if alt {
        let removed: std::collections::HashSet<fem_core::FemEntityRef> =
            group.iter().copied().collect();
        let removed_highlights: std::collections::HashSet<fem_core::FemEntityRef> =
            highlight_group.iter().copied().collect();
        selection.targets.retain(|target| !removed.contains(target));
        selection
            .highlight_targets
            .retain(|target| !removed_highlights.contains(target));

        if let Some(entity) = hover.entity {
            if let Ok(selectable) = selectable_query.get(entity) {
                if selectable.level() == level {
                    commands.entity(entity).remove::<Selected>();
                    selection.entities.retain(|&e| e != entity);
                }
            }
        }

        return;
    }

    for &target in group {
        if !selection.targets.contains(&target) {
            selection.targets.push(target);
        }
    }
    for &target in highlight_group {
        if !selection.highlight_targets.contains(&target) {
            selection.highlight_targets.push(target);
        }
    }

    // `selection.entities` / the `Selected` ECS marker only ever track the
    // one concrete pickable entity directly under the cursor — the rest of
    // a grown surface group is picked by id (already pushed into
    // `selection.targets` above), not by its own `Selectable` entity, so
    // there's nothing further to mark here.
    if let Some(entity) = hover.entity {
        if let Ok(selectable) = selectable_query.get(entity) {
            if selectable.level() == level && !selection.entities.contains(&entity) {
                commands.entity(entity).insert(Selected);

                selection.entities.push(entity);
            }
        }
    }
}
