//! A draggable slider widget built with `bevy_ui` only (no egui).
//!
//! `BorderRadius` is a **field of `Node`** in Bevy 0.18, not a separate
//! component, so all border radii are set inline inside `Node { ... }`.

use bevy::prelude::*;

// ─── public types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub enum SliderId {
    ResultStep,
    DeformScale,
    LoadMagnitude,
    /// Shell thickness or beam cross-sectional area for the section
    /// definition panel in ANALYSIS SETUP.
    SectionThickness,
    /// Face-normal angle threshold (0–90°) for Coplanar/Smooth growth.
    SurfaceAngle,
    /// Distributed load (pressure / gravity acceleration) magnitude.
    DloadMagnitude,
    /// Series playback speed: frames (or modes) per second, not physical time (0.5–10).
    PlaybackSpeed,
    /// Translation increment as a percentage of the selected part's size.
    AssemblyMovePercent,
    /// Rotation increment in degrees for assembly part editing.
    AssemblyRotationDegrees,
    /// View-only separation of a detected contact pair, as a percentage of
    /// the complete model size.
    ContactReviewSeparation,
    /// Maximum surface-to-surface distance used by contact detection, in
    /// model units.
    ContactSearchGap,
    /// Maximum deviation from aligned/opposing contact-face normals.
    ContactSearchAngle,
    /// Radius used to gather solid boundary nodes around an MPC center.
    RigidSpiderRadius,
    /// Coulomb friction coefficient for a new sliding contact pair.
    ContactFriction,
    /// Optional user-specified contact penalty factor. When automatic
    /// penalty is selected this slider value is retained but not exported.
    ContactPenaltyFactor,
}

#[derive(Component, Debug, Clone)]
pub struct SliderState {
    pub id: SliderId,
    pub min: f32,
    pub max: f32,
    pub value: f32,
    pub dragging: bool,
}

impl SliderState {
    pub fn normalized(&self) -> f32 {
        if (self.max - self.min).abs() < 1.0e-9 {
            return 0.0;
        }
        ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    pub fn set_normalized(&mut self, t: f32) {
        self.value = self.min + (self.max - self.min) * t.clamp(0.0, 1.0);
    }

    pub fn clamp_value(&mut self) {
        self.value = self.value.clamp(self.min, self.max);
    }
}

#[derive(Component)]
pub struct SliderTrack;

#[derive(Component)]
pub struct SliderThumb {
    pub track: Entity,
}

pub struct SliderConfig {
    pub width: f32,
    pub min: f32,
    pub max: f32,
    pub value: f32,
    pub label: &'static str,
    pub id: SliderId,
}

#[derive(Component)]
pub(crate) struct SliderValueText(pub SliderId);

#[derive(Component)]
pub(crate) struct SliderLabelText(pub SliderId);

#[derive(Component)]
pub(crate) struct SliderFill(pub SliderId);

const THUMB_W: f32 = 12.0;
const THUMB_H: f32 = 20.0;
const TRACK_H: f32 = 6.0;

// ─── spawn helper ────────────────────────────────────────────────────────────

pub fn spawn_slider(parent: &mut ChildSpawnerCommands, config: SliderConfig) -> Entity {
    let track_color = Color::srgba(0.22, 0.28, 0.32, 0.92);
    let fill_color = Color::srgba(0.20, 0.55, 0.72, 0.95);
    let thumb_color = Color::srgb(0.82, 0.90, 0.95);

    let initial_t = if (config.max - config.min).abs() < 1e-9 {
        0.0f32
    } else {
        ((config.value - config.min) / (config.max - config.min)).clamp(0.0, 1.0)
    };
    let thumb_left = initial_t * (config.width - THUMB_W);
    let usable_w = (config.width - THUMB_W).max(1.0);
    let fill_w = initial_t * usable_w;

    // Label row
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                width: px(config.width),
                ..default()
            },
            Name::new(format!("Slider {} label row", config.label)),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(config.label),
                SliderLabelText(config.id),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(Color::srgb(0.75, 0.82, 0.88)),
            ));
            row.spawn((
                Text::new(format!("{:.2}", config.value)),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.90, 0.94)),
                SliderValueText(config.id),
            ));
        });

    let mut track_entity = Entity::PLACEHOLDER;

    parent
        .spawn((
            Node {
                width: px(config.width),
                height: px(THUMB_H),
                align_items: AlignItems::Center,
                position_type: PositionType::Relative,
                ..default()
            },
            Name::new(format!("Slider {} container", config.label)),
        ))
        .with_children(|container| {
            // Track — border_radius is a Node FIELD, not a separate component
            let mut track_cmd = container.spawn((
                Node {
                    width: px(config.width),
                    height: px(TRACK_H),
                    position_type: PositionType::Absolute,
                    left: px(0.0),
                    border_radius: BorderRadius::all(px(3.0)),
                    ..default()
                },
                BackgroundColor(track_color),
                SliderTrack,
                SliderState {
                    id: config.id,
                    min: config.min,
                    max: config.max,
                    value: config.value,
                    dragging: false,
                },
                Name::new(format!("Slider {} track", config.label)),
            ));

            track_entity = track_cmd.id();

            // Fill strip
            track_cmd.with_children(|track| {
                track.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0.0),
                        top: px(0.0),
                        width: px(fill_w),
                        height: px(TRACK_H),
                        border_radius: BorderRadius::all(px(3.0)),
                        ..default()
                    },
                    BackgroundColor(fill_color),
                    SliderFill(config.id),
                ));
            });

            // Thumb
            container.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(thumb_left),
                    width: px(THUMB_W),
                    height: px(THUMB_H),
                    border_radius: BorderRadius::all(px(3.0)),
                    ..default()
                },
                BackgroundColor(thumb_color),
                SliderThumb {
                    track: track_entity,
                },
                Name::new(format!("Slider {} thumb", config.label)),
            ));
        });

    track_entity
}

// ─── system ──────────────────────────────────────────────────────────────────

/// Handles mouse-drag input. Visuals are synchronized separately after all
/// value writers, including automatic playback and result-range updates.
///
/// UI `Node` entities carry [`UiGlobalTransform`] (2D, physical pixels),
/// not the 3D-world [`GlobalTransform`] — there is no `GlobalTransform` on
/// a `Node` at all, so a query for one simply matches nothing and this
/// system would silently never fire for any slider. `ComputedNode::size`
/// is likewise in *physical* pixels, while `Window::cursor_position` and
/// every `Val::Px` style value here (`THUMB_W`, node widths, ...) are in
/// *logical* pixels — see [`ComputedNode::inverse_scale_factor`] — so
/// both the position and the size need that factor applied before they
/// can be compared against the cursor or written back into a `px()`
/// value. On a monitor at 100% display scaling this factor is `1.0` and
/// the distinction is invisible, which is why it's easy to miss in
/// testing but breaks on any scaled display.
pub(crate) fn update_sliders(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    popups: Query<(), With<bevy::ui_widgets::MenuPopup>>,
    mut track_query: Query<
        (
            Entity,
            &mut SliderState,
            Ref<ComputedNode>,
            &UiGlobalTransform,
        ),
        With<SliderTrack>,
    >,
) {
    // Release also ends a drag if the cursor has left the window.
    if buttons.just_released(MouseButton::Left) {
        for (_, mut state, _, _) in &mut track_query {
            state.dragging = false;
        }
    }
    if let Ok(window) = windows.single() {
        if let Some(cursor) = window.cursor_position() {
            if buttons.just_pressed(MouseButton::Left) {
                for (_, mut state, track_node, track_gt) in &mut track_query {
                    if state.id == SliderId::ResultStep && !popups.is_empty() {
                        continue;
                    }
                    if track_node.is_empty() {
                        continue;
                    }
                    let scale = track_node.inverse_scale_factor;
                    let mut size = track_node.size() * scale;
                    if state.id == SliderId::ResultStep {
                        size.y = size.y.max(THUMB_H);
                    }
                    let center = track_gt.transform_point2(Vec2::ZERO) * scale;
                    let origin = center - size * 0.5;
                    if cursor.x >= origin.x
                        && cursor.x <= origin.x + size.x
                        && cursor.y >= origin.y
                        && cursor.y <= origin.y + size.y
                    {
                        state.dragging = true;
                    }
                }
            }

            // Drag input: convert cursor X position into a normalised value.
            for (_, mut state, track_node, track_gt) in &mut track_query {
                if !state.dragging {
                    continue;
                }
                if track_node.is_empty() || (state.id == SliderId::ResultStep && !popups.is_empty())
                {
                    state.dragging = false;
                    continue;
                }

                let scale = track_node.inverse_scale_factor;
                let track_w = (track_node.size().x * scale).max(1.0);
                let track_x0 = track_gt.transform_point2(Vec2::ZERO).x * scale - track_w * 0.5;
                let usable_w = (track_w - THUMB_W).max(1.0);
                let t = ((cursor.x - track_x0 - THUMB_W * 0.5) / usable_w).clamp(0.0, 1.0);

                state.set_normalized(t);
                if state.id == SliderId::ResultStep {
                    state.value = state.value.round();
                }
            }
        }
    }
}

/// Runs after input/playback/result updates, so the thumb and displayed frame
/// cannot lag behind the contour. Layout changes also restore hidden sliders.
pub(crate) fn sync_slider_visuals(
    tracks: Query<(Entity, Ref<SliderState>, Ref<ComputedNode>), With<SliderTrack>>,
    mut thumb_query: Query<(&SliderThumb, &mut Node), Without<SliderTrack>>,
    mut fill_query: Query<(&SliderFill, &mut Node), (Without<SliderTrack>, Without<SliderThumb>)>,
    mut text_query: Query<(&SliderValueText, &mut Text)>,
) {
    for (track_entity, state, track_node) in &tracks {
        if track_node.is_empty() || (!state.is_changed() && !track_node.is_changed()) {
            continue;
        }

        let track_w = (track_node.size().x * track_node.inverse_scale_factor).max(1.0);
        let usable_w = (track_w - THUMB_W).max(1.0);
        let t = state.normalized();
        let value = state.value;
        let id = state.id;

        for (thumb, mut node) in &mut thumb_query {
            if thumb.track == track_entity {
                node.left = px(t * usable_w);
            }
        }
        for (fill, mut node) in &mut fill_query {
            if fill.0 == id {
                node.width = px(t * usable_w);
            }
        }
        for (tag, mut text) in &mut text_query {
            // The result seek bar owns its position/total and modal labels.
            if tag.0 == id && id != SliderId::ResultStep {
                **text = format!("{:.2}", value);
            }
        }
    }
}

#[cfg(test)]
#[path = "slider_tests.rs"]
mod tests;
