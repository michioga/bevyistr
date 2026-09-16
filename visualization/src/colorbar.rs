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
const BAR_W: f32 = 18.0;
const BAR_H: f32 = 200.0;
const SEG_H: f32 = BAR_H / SEGMENT_COUNT as f32;

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
/// Index used to restore the gradient after leaving a constant field.
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
                right: Val::Px(18.0),
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
                TextFont {
                    font_size: FontSize::Px(11.5),
                    ..default()
                },
                TextColor(Color::srgb(0.82, 0.90, 0.95)),
                ColorbarTitle,
            ));

            // Max value
            root.spawn((
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(10.5),
                    ..default()
                },
                TextColor(Color::srgb(0.75, 0.82, 0.88)),
                ColorbarMaxLabel,
            ));

            // Colour segments (index 0 = top = high value = red)
            root.spawn((
                Node {
                    width: Val::Px(BAR_W),
                    height: Val::Px(BAR_H),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::all(Val::Px(1.0)),
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
                            width: Val::Percent(100.0),
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
                TextFont {
                    font_size: FontSize::Px(10.5),
                    ..default()
                },
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
    range_mode: Option<Res<crate::ContourRangeMode>>,
    geometry: Option<Res<fem_core::ResultGeometry>>,
    mut root_query: Query<&mut Visibility, With<ColorbagRoot>>,
    mut title_query: Query<
        &mut Text,
        (
            With<ColorbarTitle>,
            Without<ColorbarMaxLabel>,
            Without<ColorbarMinLabel>,
        ),
    >,
    mut max_query: Query<
        &mut Text,
        (
            With<ColorbarMaxLabel>,
            Without<ColorbarTitle>,
            Without<ColorbarMinLabel>,
        ),
    >,
    mut min_query: Query<
        &mut Text,
        (
            With<ColorbarMinLabel>,
            Without<ColorbarTitle>,
            Without<ColorbarMaxLabel>,
        ),
    >,
    mut segments: Query<(&ColorbarSegment, &mut BackgroundColor)>,
) {
    if !results.is_changed()
        && !geometry.as_ref().is_some_and(|g| g.is_changed())
        && !range_mode.as_ref().is_some_and(|r| r.is_changed())
    {
        return;
    }

    let Ok(mut vis) = root_query.single_mut() else {
        return;
    };
    if geometry.as_ref().is_some_and(|g| !g.visible) {
        *vis = Visibility::Hidden;
        return;
    }

    let Some(field) = results.active_field() else {
        *vis = Visibility::Hidden;
        return;
    };

    *vis = Visibility::Visible;

    let mode = range_mode.as_deref().copied().unwrap_or_default();
    let Some((min, max)) = crate::contour_range::resolve(&results, mode) else {
        *vis = Visibility::Hidden;
        return;
    };
    let constant = (min == max).then_some(min);
    for (segment, mut color) in &mut segments {
        let t = if constant.is_some() {
            0.5
        } else {
            1.0 - segment.0 as f32 / (SEGMENT_COUNT - 1) as f32
        };
        color.set_if_neq(BackgroundColor(Color::LinearRgba(rainbow_color(t))));
    }

    if let Ok(mut text) = title_query.single_mut() {
        **text = format!(
            "{}\n{}",
            field.name(),
            if mode == crate::ContourRangeMode::AllFrames {
                "All frames"
            } else {
                "Current frame"
            }
        );
    }
    if let Ok(mut text) = max_query.single_mut() {
        **text = constant.map_or_else(|| format!("{max:.4e}"), |v| format!("Constant: {v:.4e}"));
    }
    if let Ok(mut text) = min_query.single_mut() {
        **text = if constant.is_some() {
            String::new()
        } else {
            format!("{min:.4e}")
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fem_core::{ActiveResult, ResultField, StepResult};
    #[test]
    fn constant_legend_matches_surface_and_restores_gradient() {
        let mut app = App::new();
        app.insert_resource(FemResultSet {
            by_mesh: vec![vec![StepResult {
                fields: vec![ResultField::NodeScalar {
                    name: "Velocity[2]".into(),
                    values: vec![0.; 2],
                    min: 0.,
                    max: 0.,
                }],
                ..default()
            }]],
            active: Some(ActiveResult {
                mesh_index: 0,
                step_index: 0,
                field_name: "Velocity[2]".into(),
            }),
        })
        .add_systems(Startup, spawn_colorbar)
        .add_systems(Update, update_colorbar);
        app.update();
        for (_, c) in app
            .world_mut()
            .query::<(&ColorbarSegment, &BackgroundColor)>()
            .iter(app.world())
        {
            assert_eq!(c.0, Color::LinearRgba(rainbow_color(0.5)));
        }
        let max_label = app
            .world_mut()
            .query_filtered::<Entity, With<ColorbarMaxLabel>>()
            .single(app.world())
            .unwrap();
        assert!(
            app.world()
                .get::<Text>(max_label)
                .unwrap()
                .0
                .contains("Constant")
        );
        app.world_mut().resource_mut::<FemResultSet>().by_mesh[0][0].fields[0] =
            ResultField::NodeScalar {
                name: "Velocity[2]".into(),
                values: vec![0., 0.001],
                min: 0.,
                max: 0.001,
            };
        app.update();
        assert!(
            !app.world()
                .get::<Text>(max_label)
                .unwrap()
                .0
                .contains("Constant")
        );
        for (s, c) in app
            .world_mut()
            .query::<(&ColorbarSegment, &BackgroundColor)>()
            .iter(app.world())
        {
            assert_eq!(
                c.0,
                Color::LinearRgba(rainbow_color(1. - s.0 as f32 / (SEGMENT_COUNT - 1) as f32))
            );
        }
        // Changing only the view policy must update the legend immediately.
        let future = StepResult {
            fields: vec![ResultField::NodeScalar {
                name: "Velocity[2]".into(),
                values: vec![10.],
                min: 10.,
                max: 10.,
            }],
            ..default()
        };
        app.world_mut().resource_mut::<FemResultSet>().by_mesh[0].push(future);
        app.update();
        app.insert_resource(crate::ContourRangeMode::AllFrames);
        app.update();
        assert_eq!(
            app.world().get::<Text>(max_label).unwrap().0,
            format!("{:.4e}", 10.)
        );
        app.world_mut()
            .resource_mut::<FemResultSet>()
            .active
            .as_mut()
            .unwrap()
            .step_index = 1;
        app.update();
        assert!(
            !app.world()
                .get::<Text>(max_label)
                .unwrap()
                .0
                .contains("Constant")
        );
        *app.world_mut().resource_mut::<crate::ContourRangeMode>() =
            crate::ContourRangeMode::CurrentFrame;
        app.update();
        assert!(
            app.world()
                .get::<Text>(max_label)
                .unwrap()
                .0
                .contains("Constant")
        );
        app.world_mut().resource_mut::<FemResultSet>().active = None;
        app.update();
        let vis = app
            .world_mut()
            .query_filtered::<&Visibility, With<ColorbagRoot>>()
            .single(app.world())
            .unwrap();
        assert_eq!(*vis, Visibility::Hidden);
    }
}
