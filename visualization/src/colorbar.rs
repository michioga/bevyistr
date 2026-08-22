//! Rainbow contour legend displayed as a bevy_ui overlay in the bottom-right
//! corner when a result field is active.
//!
//! The legend consists of:
//! * A field name label at the top.
//! * A vertical gradient bar built from [`SEGMENT_COUNT`] coloured
//!   [`Node`] rectangles (blue at bottom → red at top).
//! * Min/max value labels at the bottom and top of the bar.
//!
//! The whole widget is hidden when no result is active.

use bevy::prelude::*;
use fem_core::{FemResultSet, rainbow_color};

pub const SEGMENT_COUNT: usize = 20;
const BAR_W: f32  = 18.0;
const BAR_H: f32  = 200.0;
const SEG_H: f32  = BAR_H / SEGMENT_COUNT as f32;

// ─── components ──────────────────────────────────────────────────────────────

#[derive(Component)]
pub struct ColorbagRoot;

#[derive(Component)]
pub struct ColorbarTitle;

#[derive(Component)]
pub struct ColorbarMaxLabel;

#[derive(Component)]
pub struct ColorbarMinLabel;

/// Marks one colour segment of the colorbar gradient.
/// The rainbow colour is computed at spawn time and baked into
/// `BackgroundColor`, so no runtime index lookup is needed.
#[derive(Component)]
#[allow(dead_code)]
pub struct ColorbarSegment(pub usize);

// ─── spawn ───────────────────────────────────────────────────────────────────

/// Spawns the colorbar overlay (initially hidden).
pub fn spawn_colorbar(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right:  Val::Px(18.0),
                bottom: Val::Px(18.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0),
                ..default()
            },
            Visibility::Hidden,
            ColorbagRoot,
            Name::new("ColorbarRoot"),
        ))
        .with_children(|root| {
            // Field name
            root.spawn((
                Text::new(""),
                TextFont { font_size: FontSize::Px(11.5), ..default() },
                TextColor(Color::srgb(0.82, 0.90, 0.95)),
                ColorbarTitle,
            ));

            // Max value
            root.spawn((
                Text::new(""),
                TextFont { font_size: FontSize::Px(10.5), ..default() },
                TextColor(Color::srgb(0.75, 0.82, 0.88)),
                ColorbarMaxLabel,
            ));

            // Colour segments (index 0 = top = high value = red)
            root.spawn((
                Node {
                    width:          Val::Px(BAR_W),
                    height:         Val::Px(BAR_H),
                    flex_direction: FlexDirection::Column,
                    border:         UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(Color::srgba(0.30, 0.36, 0.40, 0.70)),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
            ))
            .with_children(|bar| {
                for i in 0..SEGMENT_COUNT {
                    // i=0 → top → t=1.0 (red), i=N-1 → bottom → t=0.0 (blue)
                    let t = 1.0 - i as f32 / (SEGMENT_COUNT - 1) as f32;
                    let c = rainbow_color(t);

                    bar.spawn((
                        Node {
                            width:  Val::Percent(100.0),
                            height: Val::Px(SEG_H),
                            ..default()
                        },
                        BackgroundColor(Color::LinearRgba(c)),
                        ColorbarSegment(i),
                    ));
                }
            });

            // Min value
            root.spawn((
                Text::new(""),
                TextFont { font_size: FontSize::Px(10.5), ..default() },
                TextColor(Color::srgb(0.75, 0.82, 0.88)),
                ColorbarMinLabel,
            ));
        });
}

// ─── update system ───────────────────────────────────────────────────────────

/// Shows/hides the colorbar and updates its labels whenever the active
/// result field changes.
pub fn update_colorbar(
    results: Res<FemResultSet>,
    mut root_query:  Query<&mut Visibility, With<ColorbagRoot>>,
    mut title_query: Query<&mut Text, (With<ColorbarTitle>, Without<ColorbarMaxLabel>, Without<ColorbarMinLabel>)>,
    mut max_query:   Query<&mut Text, (With<ColorbarMaxLabel>, Without<ColorbarTitle>, Without<ColorbarMinLabel>)>,
    mut min_query:   Query<&mut Text, (With<ColorbarMinLabel>, Without<ColorbarTitle>, Without<ColorbarMaxLabel>)>,
) {
    if !results.is_changed() {
        return;
    }

    let Ok(mut vis) = root_query.single_mut() else { return; };

    let Some(field) = results.active_field() else {
        *vis = Visibility::Hidden;
        return;
    };

    *vis = Visibility::Visible;

    let (name, min, max) = match field {
        fem_core::ResultField::NodeScalar { name, min, max, .. } => {
            (name.as_str(), *min, *max)
        }
        fem_core::ResultField::NodeVector { name, min_mag, max_mag, .. } => {
            (name.as_str(), *min_mag, *max_mag)
        }
        fem_core::ResultField::ElementScalar { name, min, max, .. } => {
            (name.as_str(), *min, *max)
        }
    };

    if let Ok(mut text) = title_query.single_mut() {
        **text = name.to_string();
    }
    if let Ok(mut text) = max_query.single_mut() {
        **text = format!("{max:.4e}");
    }
    if let Ok(mut text) = min_query.single_mut() {
        **text = format!("{min:.4e}");
    }
}
