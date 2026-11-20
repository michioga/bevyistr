//! Opt-in CPU measurements of the viewport selection systems with an external MSH.
use std::{path::PathBuf, time::Instant};

use bevy::{
    camera::{ComputedCameraValues, RenderTargetInfo},
    prelude::*,
};
use fem_core::{
    FemModel, HoverPreviewTargets, MainViewportCamera, SelectionFilter, SelectionLevel,
};
use interaction::{HoverResult, InteractionPlugin, InteractionSystems};
use selection::{ClickSequence, SelectionPlugin, SelectionState};

use crate::selection_ui::{
    SurfaceSelectionMode, SurfaceSelectionSettings, update_hover_preview_group,
};

fn cursor(app: &mut App, window: Entity, position: Vec2) {
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(position));
}

fn update_us(app: &mut App) -> f64 {
    let start = Instant::now();
    app.update();
    start.elapsed().as_secs_f64() * 1e6
}

fn report(label: &str, times: &mut [f64]) {
    times.sort_by(f64::total_cmp);
    let median = (times[(times.len() - 1) / 2] + times[times.len() / 2]) * 0.5;
    let p95 = times[(times.len() * 95).div_ceil(100) - 1];
    println!(
        "{label} samples={} median_us={median:.3} p95_us={p95:.3}",
        times.len()
    );
}

#[test]
#[ignore = "Read-only CPU measurement; set BEVYISTR_BENCH_MESH to a FrontISTR solid MSH"]
fn real_selection_pipeline_baseline() {
    let path =
        PathBuf::from(std::env::var_os("BEVYISTR_BENCH_MESH").expect("Set BEVYISTR_BENCH_MESH"));
    let mesh = hecmw::load_mesh_file(&path).expect("Mesh loading failed");
    let (min, max) = mesh.bounds().expect("Empty mesh");
    println!(
        "INPUT {} nodes={} elements={} boundary_faces={}",
        path.display(),
        mesh.nodes.len(),
        mesh.elements.len(),
        mesh.cached_boundary_faces().len()
    );

    // The camera and cursor are deterministic, and no OS window, renderer or GPU is required.
    let mut app = App::new();
    app.init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Time>()
        .init_resource::<SurfaceSelectionSettings>()
        .init_resource::<HoverPreviewTargets>()
        .insert_resource(FemModel::single_mesh("selection measurement", mesh))
        .add_plugins((InteractionPlugin, picking::PickingPlugin, SelectionPlugin))
        .add_systems(
            Update,
            update_hover_preview_group.in_set(InteractionSystems::Preview),
        );
    let window = app
        .world_mut()
        .spawn(Window {
            resolution: (800, 600).into(),
            ..default()
        })
        .id();
    let focus = (min + max) * 0.5;
    let radius = (max - min).length();
    app.world_mut().spawn((
        MainViewportCamera,
        Camera {
            computed: ComputedCameraValues {
                clip_from_view: bevy::math::proj::perspective_infinite_reverse(
                    1.0,
                    800.0 / 600.0,
                    radius * 0.001,
                ),
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(800, 600),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        },
        GlobalTransform::from_translation(focus + Vec3::Z * radius),
    ));
    app.update();

    let samples: Vec<_> = (0..256)
        .map(|i| {
            Vec2::new(
                250.0 + (i % 16) as f32 * 20.0,
                150.0 + (i / 16) as f32 * 20.0,
            )
        })
        .collect();
    let mut face_sample = None;
    for (level, mode) in [
        (SelectionLevel::Node, SurfaceSelectionMode::Single),
        (SelectionLevel::Edge, SurfaceSelectionMode::Single),
        (SelectionLevel::Face, SurfaceSelectionMode::Single),
        (SelectionLevel::Element, SurfaceSelectionMode::Single),
        (SelectionLevel::Face, SurfaceSelectionMode::Smooth),
    ] {
        *app.world_mut().resource_mut::<SelectionFilter>() = SelectionFilter::new(level);
        app.world_mut()
            .resource_mut::<SurfaceSelectionSettings>()
            .mode = mode;
        let mut hit_times = Vec::new();
        let mut miss_times = Vec::new();
        for &position in &samples {
            cursor(&mut app, window, position);
            let elapsed = update_us(&mut app);
            if let Some(hit) = app.world().resource::<HoverResult>().hit {
                assert_eq!(hit.level(), level);
                assert!(
                    !app.world()
                        .resource::<HoverPreviewTargets>()
                        .targets
                        .is_empty()
                );
                hit_times.push(elapsed);
                if (level, mode) == (SelectionLevel::Face, SurfaceSelectionMode::Single) {
                    face_sample.get_or_insert(position);
                }
            } else {
                miss_times.push(elapsed);
            }
        }
        for (kind, times) in [
            ("hit", hit_times.as_mut_slice()),
            ("miss", miss_times.as_mut_slice()),
        ] {
            if !times.is_empty() {
                report(&format!("HOVER {level:?} {mode:?} {kind}"), times);
            }
        }
    }

    *app.world_mut().resource_mut::<SelectionFilter>() = SelectionFilter::new(SelectionLevel::Face);
    app.world_mut()
        .resource_mut::<SurfaceSelectionSettings>()
        .mode = SurfaceSelectionMode::Single;
    cursor(
        &mut app,
        window,
        face_sample.expect("Camera did not hit a face"),
    );
    app.update();
    let target = app.world().resource::<HoverResult>().target().unwrap();
    let mut times = Vec::new();
    let mut count = None;
    for _ in 0..25 {
        let mut clicks = app.world_mut().resource_mut::<ClickSequence>();
        clicks.reset();
        clicks.register(target, 0.0);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        times.push(update_us(&mut app));
        let selected = app.world().resource::<SelectionState>().len();
        assert!(selected > 1, "Expected connected boundary selection");
        assert_eq!(selected, *count.get_or_insert(selected));
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        buttons.release(MouseButton::Left);
        buttons.clear();
    }
    report(
        &format!("DOUBLE_CLICK Face selected={}", count.unwrap()),
        &mut times,
    );
}
