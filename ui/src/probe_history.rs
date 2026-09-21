//! Cached histories with one shared range, for the pin or explicit comparison.
use crate::{
    layout::SidebarPage,
    result_probe_pin::{ProbePin, Target},
};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use fem_core::{FemResultSet, StepResult};
use visualization::VisualizationSettings;

const WIDTH: usize = 512;
const HEIGHT: usize = 256;
const PAD: f64 = 12.0;
const CURVE: [u8; 4] = [65, 200, 235, 255];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Time,
    Frame,
}

struct History {
    x: Vec<f64>,
    values: Vec<Option<f32>>,
    axis: Axis,
    range: Option<(f64, f64)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use fem_core::{NodeId, ResultField};

    fn target() -> Target {
        Target::Node {
            index: 0,
            id: NodeId(71),
        }
    }
    fn step(time: f32, value: Option<f32>) -> StepResult {
        StepResult {
            time,
            fields: value
                .map(|v| ResultField::NodeScalar {
                    name: "P".into(),
                    values: vec![v],
                    min: v,
                    max: v,
                })
                .into_iter()
                .collect(),
            ..default()
        }
    }

    #[test]
    fn irregular_times_use_real_spacing_and_fallback_never_reorders_modes() {
        let history = History::build(
            &[step(0., Some(2.)), step(0.1, Some(4.)), step(1., Some(6.))],
            target(),
            "P",
        );
        assert_eq!(history.axis, Axis::Time);
        let fraction = (history.x_pixel(1).unwrap() - PAD) / (WIDTH as f64 - 1. - 2. * PAD);
        assert!((fraction - 0.1).abs() < 1e-6);
        for times in [[0., 0., 0.], [0., 2., 1.], [0., f32::NAN, 1.]] {
            let history = History::build(&times.map(|t| step(t, Some(2.))), target(), "P");
            assert_eq!(history.axis, Axis::Frame);
            assert_eq!(history.x, vec![1., 2., 3.]);
        }
    }

    #[test]
    fn missing_samples_break_the_curve_and_extremes_do_not_overflow() {
        let history = History::build(
            &[step(0., Some(0.)), step(1., None), step(2., Some(10.))],
            target(),
            "P",
        );
        assert_eq!(history.range, Some((0., 10.)));
        assert_eq!(history.point(1), None);
        let pixels = history.pixels();
        for y in 0..HEIGHT {
            let offset = (y * WIDTH + WIDTH / 2) * 4;
            assert_ne!(&pixels[offset..offset + 4], CURVE.as_slice());
        }
        let history = History::build(
            &[
                step(-f32::MAX, Some(-f32::MAX)),
                step(f32::MAX, Some(f32::MAX)),
            ],
            target(),
            "P",
        );
        assert!(history.point(0).unwrap().is_finite());
        assert!(history.point(1).unwrap().is_finite());
    }

    #[test]
    fn constant_single_and_unavailable_histories_preserve_values() {
        let single = History::build(&[step(5., Some(-7.))], target(), "P");
        assert_eq!(single.axis, Axis::Frame);
        assert_eq!(single.range, Some((-7., -7.)));
        assert_eq!(
            single.point(0),
            Some(Vec2::new((WIDTH - 1) as f32 / 2., (HEIGHT - 1) as f32 / 2.))
        );
        assert!(single.pixels().chunks_exact(4).any(|p| p == CURVE));
        let empty = History::build(&[step(0., None), step(1., Some(f32::NAN))], target(), "P");
        assert_eq!(empty.range, None);
        assert!(empty.values.iter().all(Option::is_none));
        assert!(!empty.pixels().chunks_exact(4).any(|p| p == CURVE));
    }

    #[test]
    fn ui_caches_one_plot_per_target_and_field_and_only_moves_marker_on_playback() {
        let mut app = App::new();
        app.insert_resource(SidebarPage::Results)
            .insert_resource(ProbePin::for_test(1, target()))
            .insert_resource(FemResultSet {
                by_mesh: vec![
                    vec![step(0., Some(999.))],
                    vec![step(0., Some(2.)), step(0.25, Some(4.)), step(1., None)],
                ],
                ..default()
            })
            .insert_resource(VisualizationSettings {
                contour: Some(visualization::ContourSettings {
                    mesh_index: 0,
                    step_index: 0,
                    field_name: "P".into(),
                    show_deformation: true,
                    displacement_field: "U".into(),
                    deformation_scale: 20.,
                }),
                ..default()
            })
            .init_resource::<HistoryCache>()
            .init_resource::<Assets<Image>>()
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(spawn);
            })
            .add_systems(Update, update);
        app.update();
        let image_node = app
            .world_mut()
            .query_filtered::<&ImageNode, With<HistoryPlot>>()
            .single(app.world())
            .unwrap();
        assert_eq!(image_node.image_mode, NodeImageMode::Stretch);
        assert_eq!(
            app.world()
                .resource::<HistoryCache>()
                .history
                .as_ref()
                .unwrap()
                .values,
            vec![Some(2.), Some(4.), None]
        );
        let handle = app
            .world()
            .resource::<HistoryCache>()
            .image
            .clone()
            .unwrap();
        let plot = app
            .world()
            .resource::<Assets<Image>>()
            .get(&handle)
            .unwrap()
            .data
            .clone();
        for frame in [1, 2, 0] {
            app.world_mut()
                .resource_mut::<VisualizationSettings>()
                .contour
                .as_mut()
                .unwrap()
                .step_index = frame;
            app.update();
            assert_eq!(app.world().resource::<HistoryCache>().builds, 1);
            assert_eq!(
                app.world()
                    .resource::<Assets<Image>>()
                    .get(&handle)
                    .unwrap()
                    .data,
                plot
            );
        }
        app.world_mut()
            .resource_mut::<VisualizationSettings>()
            .contour
            .as_mut()
            .unwrap()
            .field_name = "missing".into();
        app.update();
        assert_eq!(app.world().resource::<HistoryCache>().builds, 2);
        assert_eq!(
            app.world()
                .resource::<HistoryCache>()
                .history
                .as_ref()
                .unwrap()
                .range,
            None
        );
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
        app.world_mut().resource_mut::<ProbePin>().clear();
        app.update();
        assert!(app.world().resource::<HistoryCache>().history.is_none());
        let root = app
            .world_mut()
            .query_filtered::<&Node, With<HistoryRoot>>()
            .single(app.world())
            .unwrap();
        assert_eq!(root.display, Display::None);
    }

    #[test]
    fn endpoint_markers_match_stretched_texture_pixels_at_different_panel_sizes() {
        let history = History::build(&[step(0., Some(0.)), step(1., Some(10.))], target(), "P");
        for frame in [0, 1] {
            let source = history.point(frame).unwrap();
            let point = marker_position(&Marker::Point, &history, frame).unwrap();
            let line = marker_position(&Marker::Line, &history, frame).unwrap();
            assert_eq!(point.x, line.x);
            assert_eq!(line.y, 0.0);
            for size in [
                Vec2::new(256., 128.),
                Vec2::new(294., 128.),
                Vec2::new(420., 160.),
            ] {
                let displayed = point * size;
                // Stretch fills the node: a texel center has exactly this position.
                let expected =
                    (source + Vec2::splat(0.5)) / Vec2::new(WIDTH as f32, HEIGHT as f32) * size;
                assert!(displayed.distance(expected) < 0.001);
                assert!(displayed.x >= 3. && displayed.x + 3. <= size.x);
                assert!(displayed.y >= 3. && displayed.y + 3. <= size.y);
            }
        }
        assert!(marker_position(&Marker::Point, &history, 2).is_none());
    }

    #[test]
    fn comparison_uses_shared_range_stable_colors_and_cached_curves_without_a_pin() {
        use crate::probe_comparison::{COLORS, Comparison, Entry};
        let mut comparison = Comparison::default();
        comparison.entries = vec![
            Entry {
                slot: 1,
                part: 0,
                target: target(),
            },
            Entry {
                slot: 3,
                part: 1,
                target: target(),
            },
        ];
        let mut app = App::new();
        app.insert_resource(SidebarPage::Results)
            .init_resource::<ProbePin>() // clearing the current pin must not hide comparison
            .insert_resource(comparison)
            .insert_resource(FemResultSet {
                by_mesh: vec![
                    vec![step(0., Some(-10.)), step(1., Some(0.)), step(2., Some(5.))],
                    vec![step(0., Some(20.)), step(1., None), step(2., Some(40.))],
                ],
                ..default()
            })
            .insert_resource(VisualizationSettings {
                contour: Some(visualization::ContourSettings {
                    mesh_index: 0,
                    step_index: 0,
                    field_name: "P".into(),
                    show_deformation: true,
                    displacement_field: "U".into(),
                    deformation_scale: 1.,
                }),
                ..default()
            })
            .init_resource::<HistoryCache>()
            .init_resource::<Assets<Image>>()
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(spawn);
            })
            .add_systems(Update, update);
        app.update();
        let cache = app.world().resource::<HistoryCache>();
        let first = cache.history.as_ref().unwrap();
        assert_eq!(first.range, Some((-10., 40.)));
        assert_eq!(cache.others[0].range, first.range);
        assert_eq!(cache.others[0].values, vec![Some(20.), None, Some(40.)]);
        let image = cache.image.clone().unwrap();
        let pixels = app
            .world()
            .resource::<Assets<Image>>()
            .get(&image)
            .unwrap()
            .data
            .as_ref()
            .unwrap();
        for color in [COLORS[1], COLORS[3]] {
            assert!(pixels.chunks_exact(4).any(|p| p == color));
        }
        assert!(!pixels.chunks_exact(4).any(|p| p == COLORS[0]));
        for frame in [1, 2, 0] {
            app.world_mut()
                .resource_mut::<VisualizationSettings>()
                .contour
                .as_mut()
                .unwrap()
                .step_index = frame;
            app.world_mut().resource_mut::<FemResultSet>().active = Some(fem_core::ActiveResult {
                mesh_index: 0,
                step_index: frame,
                field_name: "P".into(),
            });
            app.insert_resource(ProbePin::for_test(0, target()));
            app.update();
            assert_eq!(app.world().resource::<HistoryCache>().builds, 1);
        }
        // Comparison uses the vertical current-frame marker, not one misleading point.
        let mut marker_nodes = app.world_mut().query::<(&Marker, &Node)>();
        assert!(
            marker_nodes
                .iter(app.world())
                .any(|(kind, node)| matches!(kind, Marker::Point) && node.display == Display::None)
        );
        app.world_mut()
            .resource_mut::<Comparison>()
            .entries
            .remove(0);
        app.update();
        assert_eq!(
            app.world()
                .resource::<HistoryCache>()
                .history
                .as_ref()
                .unwrap()
                .range,
            Some((20., 40.))
        );
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
    }
}

impl History {
    fn build(steps: &[StepResult], target: Target, field: &str) -> Self {
        // Time=0 is also the reader's missing-time default. Never invent times,
        // sort eigenmodes, or reorder frames when times repeat or run backwards.
        let timed = steps.len() > 1
            && steps.iter().all(|s| s.time.is_finite())
            && steps.windows(2).all(|s| s[1].time > s[0].time);
        let axis = if timed { Axis::Time } else { Axis::Frame };
        let x = steps
            .iter()
            .enumerate()
            .map(|(i, s)| if timed { s.time as f64 } else { (i + 1) as f64 })
            .collect();
        let values: Vec<_> = crate::probe_history_data::samples(steps, target, field)
            .into_iter()
            .map(|sample| sample.value)
            .collect();
        let range = values.iter().flatten().fold(None, |range, &v| {
            let v = v as f64;
            Some(range.map_or((v, v), |(lo, hi): (f64, f64)| (lo.min(v), hi.max(v))))
        });
        Self {
            x,
            values,
            axis,
            range,
        }
    }

    fn x_pixel(&self, frame: usize) -> Option<f64> {
        let value = *self.x.get(frame)?;
        let lo = *self.x.first()?;
        let hi = *self.x.last()?;
        let fraction = if lo == hi {
            0.5
        } else {
            (value - lo) / (hi - lo)
        };
        Some(PAD + fraction * (WIDTH as f64 - 1.0 - 2.0 * PAD))
    }

    fn point(&self, frame: usize) -> Option<Vec2> {
        let value = self.values.get(frame).copied().flatten()? as f64;
        let (lo, hi) = self.range?;
        let fraction = if lo == hi {
            0.5
        } else {
            (value - lo) / (hi - lo)
        };
        Some(Vec2::new(
            self.x_pixel(frame)? as f32,
            (PAD + (1.0 - fraction) * (HEIGHT as f64 - 1.0 - 2.0 * PAD)) as f32,
        ))
    }

    fn pixels(&self) -> Vec<u8> {
        let mut pixels = [12, 19, 24, 255].repeat(WIDTH * HEIGHT);
        for t in [0.0, 0.5, 1.0] {
            let y = PAD as f32 + t * (HEIGHT as f32 - 1.0 - 2.0 * PAD as f32);
            line(
                &mut pixels,
                Vec2::new(PAD as f32, y),
                Vec2::new(WIDTH as f32 - 1.0 - PAD as f32, y),
                [45, 60, 69, 255],
            );
        }
        self.draw(&mut pixels, CURVE);
        pixels
    }

    fn draw(&self, pixels: &mut [u8], color: [u8; 4]) {
        for i in 0..self.values.len() {
            let Some(point) = self.point(i) else { continue };
            if i > 0 {
                if let Some(previous) = self.point(i - 1) {
                    line(pixels, previous, point, color);
                }
            }
            // Also show isolated and single-frame samples; never bridge gaps.
            for dx in -2..=2 {
                for dy in -2..=2 {
                    pixel(
                        pixels,
                        point.x.round() as i32 + dx,
                        point.y.round() as i32 + dy,
                        color,
                    );
                }
            }
        }
    }
}

fn pixel(pixels: &mut [u8], x: i32, y: i32, color: [u8; 4]) {
    if x >= 0 && x < WIDTH as i32 && y >= 0 && y < HEIGHT as i32 {
        let offset = (y as usize * WIDTH + x as usize) * 4;
        pixels[offset..offset + 4].copy_from_slice(&color);
    }
}

fn line(pixels: &mut [u8], start: Vec2, end: Vec2, color: [u8; 4]) {
    let count = (end - start).abs().max_element().ceil() as usize;
    for i in 0..=count {
        let p = start.lerp(end, i as f32 / count.max(1) as f32);
        pixel(pixels, p.x.round() as i32, p.y.round() as i32, color);
    }
}

#[derive(PartialEq, Eq)]
struct Key {
    selection: (u64, usize, Target),
    field: String,
    comparison: Vec<crate::probe_comparison::Entry>,
}

#[derive(Resource, Default)]
pub(crate) struct HistoryCache {
    key: Option<Key>,
    history: Option<History>,
    others: Vec<History>,
    image: Option<Handle<Image>>,
    #[cfg(test)]
    builds: usize,
}

#[derive(Component)]
pub(crate) struct HistoryRoot;
#[derive(Component)]
pub(crate) struct HistoryPlot;
#[derive(Component)]
pub(crate) enum Label {
    Top,
    Bottom,
    Axis,
}
#[derive(Component)]
pub(crate) enum Marker {
    Line,
    Point,
}

/// Fractions of the same rectangle filled by the stretched image. Texture
/// coordinates address pixel centers, not the outermost image edges.
fn marker_position(kind: &Marker, history: &History, frame: usize) -> Option<Vec2> {
    match kind {
        Marker::Line => Some(Vec2::new(
            (history.x_pixel(frame)? as f32 + 0.5) / WIDTH as f32,
            0.0,
        )),
        Marker::Point => Some(
            (history.point(frame)? + Vec2::splat(0.5)) / Vec2::new(WIDTH as f32, HEIGHT as f32),
        ),
    }
}

fn label(parent: &mut ChildSpawnerCommands, kind: Label) {
    parent.spawn((
        kind,
        Pickable::IGNORE,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(Color::srgb(0.8, 0.88, 0.92)),
    ));
}

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            HistoryRoot,
            Node {
                display: Display::None,
                width: percent(100),
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                margin: UiRect::vertical(px(5)),
                ..default()
            },
        ))
        .with_children(|root| {
            label(root, Label::Top);
            root.spawn((
                HistoryPlot,
                // Auto aspect-fits the image inside the node, while absolute
                // children use the full node. Stretch keeps their coordinates aligned.
                ImageNode {
                    image_mode: NodeImageMode::Stretch,
                    ..default()
                },
                Pickable::IGNORE,
                Node {
                    width: percent(100),
                    height: px(128),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(|plot| {
                for marker in [Marker::Line, Marker::Point] {
                    let point = matches!(marker, Marker::Point);
                    plot.spawn((
                        marker,
                        Pickable::IGNORE,
                        Node {
                            position_type: PositionType::Absolute,
                            display: Display::None,
                            width: px(if point { 6 } else { 1 }),
                            height: if point { px(6) } else { percent(100) },
                            margin: if point {
                                UiRect {
                                    left: px(-3),
                                    top: px(-3),
                                    ..default()
                                }
                            } else {
                                UiRect::default()
                            },
                            ..default()
                        },
                        BackgroundColor(Color::srgb(1.0, 0.8, 0.15)),
                    ));
                }
            });
            label(root, Label::Bottom);
            label(root, Label::Axis);
        });
}

pub(crate) fn update(
    page: Res<SidebarPage>,
    pin: Res<ProbePin>,
    results: Res<FemResultSet>,
    settings: Res<VisualizationSettings>,
    comparison: Option<Res<crate::probe_comparison::Comparison>>,
    mut cache: ResMut<HistoryCache>,
    mut images: ResMut<Assets<Image>>,
    mut roots: Query<&mut Node, (With<HistoryRoot>, Without<Marker>)>,
    mut plots: Query<&mut ImageNode, With<HistoryPlot>>,
    mut labels: Query<(&Label, &mut Text)>,
    mut markers: Query<(&Marker, &mut Node), Without<HistoryRoot>>,
) {
    let entries = comparison
        .as_ref()
        .map(|c| c.entries.as_slice())
        .unwrap_or(&[]);
    let selection = entries
        .first()
        .map(|e| (comparison.as_ref().unwrap().revision, e.part, e.target))
        .or_else(|| pin.selection())
        .filter(|_| *page == SidebarPage::Results);
    let contour = settings.contour.as_ref();
    let visible = selection.is_some() && contour.is_some();
    for mut root in &mut roots {
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if root.display != display {
            root.display = display;
        }
    }
    let (Some(selection), Some(contour)) = (selection, contour) else {
        cache.key = None;
        cache.history = None;
        cache.others.clear();
        return;
    };
    let key = Key {
        selection,
        field: contour.field_name.clone(),
        comparison: entries.to_vec(),
    };
    if cache.key.as_ref() != Some(&key) {
        let steps = results
            .by_mesh
            .get(selection.1)
            .map_or(&[][..], Vec::as_slice);
        let mut history = History::build(steps, selection.2, &key.field);
        let mut others: Vec<_> = entries
            .iter()
            .skip(1)
            .map(|e| {
                History::build(
                    results.by_mesh.get(e.part).map_or(&[][..], Vec::as_slice),
                    e.target,
                    &key.field,
                )
            })
            .collect();
        let range = others
            .iter()
            .filter_map(|h| h.range)
            .fold(history.range, |range, (lo, hi)| {
                Some(range.map_or((lo, hi), |(a, b)| (a.min(lo), b.max(hi))))
            });
        history.range = range;
        for other in &mut others {
            other.range = range;
        }
        let mut pixels = history.pixels();
        if let Some(first) = entries.first() {
            history.draw(&mut pixels, crate::probe_comparison::COLORS[first.slot]);
        }
        for (other, entry) in others.iter().zip(entries.iter().skip(1)) {
            other.draw(&mut pixels, crate::probe_comparison::COLORS[entry.slot]);
        }
        let image = Image::new(
            Extent3d {
                width: WIDTH as u32,
                height: HEIGHT as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        if let Some(mut existing) = cache.image.as_ref().and_then(|h| images.get_mut(h)) {
            *existing = image;
        } else {
            cache.image = Some(images.add(image));
        }
        let count = history.values.iter().flatten().count()
            + others
                .iter()
                .map(|h| h.values.iter().flatten().count())
                .sum::<usize>();
        let samples = steps.len() * (others.len() + 1);
        for (kind, mut text) in &mut labels {
            let value = match kind {
                Label::Top => match history.range {
                    Some((lo, hi)) if lo == hi => format!("HISTORY | Constant: {lo:.6e}"),
                    Some((_, hi)) => format!("HISTORY | Max: {hi:.6e}"),
                    None => "HISTORY | No finite values for this target/field".into(),
                },
                Label::Bottom => match history.range {
                    Some((lo, _)) => {
                        format!("Min: {lo:.6e} | {count}/{} samples | model units", samples)
                    }
                    None => format!("0/{samples} samples; missing values are not zero"),
                },
                Label::Axis => {
                    let lo = history.x.first().copied().unwrap_or(0.0);
                    let hi = history.x.last().copied().unwrap_or(0.0);
                    match history.axis {
                        Axis::Time => format!(
                            "Time / load factor: {lo:.4e} to {hi:.4e}\nYellow: current frame | gaps: unavailable"
                        ),
                        Axis::Frame if steps.len() == 1 => {
                            "Frame: 1 (single sample)\nYellow: current frame".into()
                        }
                        Axis::Frame => format!(
                            "Frame: {lo:.0} to {hi:.0} (time missing/non-increasing)\nYellow: current frame | gaps: unavailable"
                        ),
                    }
                }
            };
            text.set_if_neq(Text::new(value));
        }
        cache.history = Some(history);
        cache.others = others;
        cache.key = Some(key);
        #[cfg(test)]
        {
            cache.builds += 1;
        }
    }
    for mut plot in &mut plots {
        if let Some(image) = &cache.image {
            if plot.image != *image {
                plot.image = image.clone();
            }
        }
    }
    let history = cache.history.as_ref().unwrap();
    for (kind, mut node) in &mut markers {
        let point = if !entries.is_empty() && matches!(kind, Marker::Point) {
            None
        } else {
            marker_position(kind, history, contour.step_index)
        };
        let mut next = node.clone();
        next.display = if point.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if let Some(p) = point {
            next.left = percent(100.0 * p.x);
            next.top = percent(100.0 * p.y);
        }
        node.set_if_neq(next);
    }
}
