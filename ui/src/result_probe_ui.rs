//! Non-mutating, paused-frame result probe. Geometry queries are cached, not
//! rebuilt for every cursor move, and never reference the hidden pre-model.
use crate::{layout::SidebarPage, results_ui::PlaybackState};
use bevy::{picking::Pickable, prelude::*, window::PrimaryWindow};
use fem_core::{
    FemModel, FemModelVersion, FemResultSet, MainViewportCamera, ResultGeometry, UiPointerState,
};
use visualization::{
    VisualizationSettings,
    result_probe::{ProbeHit, ProbeSurface},
};

#[derive(Component)]
enum Overlay {
    Tooltip,
    Marker,
}
#[derive(Component)]
struct ProbeText;
#[derive(Default)]
struct ProbeCache {
    surfaces: Option<Vec<(usize, ProbeSurface)>>,
}

pub(crate) fn register(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(
        PostUpdate,
        update.after(bevy::transform::TransformSystems::Propagate),
    );
}

fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Overlay::Tooltip,
            Pickable::IGNORE,
            GlobalZIndex(190),
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                width: px(280),
                padding: UiRect::all(px(8)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.025, 0.04, 0.055, 0.96)),
            BorderColor::all(Color::srgb(0.5, 0.8, 0.9)),
        ))
        .with_child((
            ProbeText,
            Pickable::IGNORE,
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ));
    commands.spawn((
        Overlay::Marker,
        Pickable::IGNORE,
        GlobalZIndex(189),
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            width: px(8),
            height: px(8),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        BorderColor::all(Color::WHITE),
        BackgroundColor(Color::srgb(1.0, 0.6, 0.05)),
    ));
}

fn describe(part: usize, step: &fem_core::StepResult, field: &str, hit: &ProbeHit) -> String {
    let target = if let Some(node) = hit.node {
        format!("Node {} | nearest triangle node", node.0)
    } else {
        "Element value | no nodal averaging".into()
    };
    let element = hit
        .element
        .map_or_else(|| "n/a".into(), |e| e.0.to_string());
    let value = hit
        .value
        .map_or_else(|| "unavailable".into(), |v| format!("{v:.6e}"));
    let quantity = if matches!(
        step.field_by_name(field),
        Some(fem_core::ResultField::NodeVector { .. })
    ) {
        "Magnitude"
    } else {
        "Value"
    };
    format!(
        "Part {} | Element {element}\n{target}\n{field}\n{quantity}: {value}\nStep {} | Time {:.6e}\nResult/model units",
        part + 1,
        step.step,
        step.time
    )
}

fn update(
    page: Res<SidebarPage>,
    playback: Res<PlaybackState>,
    pointer: Res<UiPointerState>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<MainViewportCamera>>,
    model: Option<Res<FemModel>>,
    geometry: Res<ResultGeometry>,
    version: Res<FemModelVersion>,
    results: Res<FemResultSet>,
    settings: Res<VisualizationSettings>,
    mut cache: Local<ProbeCache>,
    mut overlays: Query<(&Overlay, &mut Node)>,
    mut text: Query<&mut Text, With<ProbeText>>,
) {
    for (_, mut node) in &mut overlays {
        node.display = Display::None;
    }
    // Invalidate even while playback/UI suppresses querying. Rebuild lazily on
    // the next eligible hover, so playback never constructs a per-frame BVH.
    if results.is_changed()
        || settings.is_changed()
        || geometry.is_changed()
        || version.is_changed()
        || model.as_ref().is_some_and(|m| m.is_changed())
    {
        cache.surfaces = None;
    }
    if *page != SidebarPage::Results
        || !geometry.visible
        || playback.playing
        || pointer.over_ui
        || mouse.get_pressed().next().is_some()
    {
        return;
    }
    let Some(contour) = &settings.contour else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((camera, transform)) = camera.single() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(transform, cursor) else {
        return;
    };
    let Some(model) = geometry.model.as_ref().or(model.as_deref()) else {
        return;
    };
    if cache.surfaces.is_none() {
        cache.surfaces = Some(
            model
                .meshes
                .iter()
                .enumerate()
                .filter_map(|(i, mesh)| {
                    let step = results.by_mesh.get(i)?.get(contour.step_index)?;
                    Some((i, ProbeSurface::build(mesh, step, contour)))
                })
                .collect(),
        );
    }
    let mut best: Option<(usize, ProbeHit)> = None;
    for (i, surface) in cache.surfaces.as_ref().unwrap() {
        let Some(step) = results
            .by_mesh
            .get(*i)
            .and_then(|s| s.get(contour.step_index))
        else {
            continue;
        };
        let Some(field) = step.field_by_name(&contour.field_name) else {
            continue;
        };
        if let Some(hit) = surface.pick(ray.origin, *ray.direction, field) {
            if best
                .as_ref()
                .is_none_or(|(_, current)| hit.distance < current.distance)
            {
                best = Some((*i, hit));
            }
        }
    }
    let Some((part, hit)) = best else {
        return;
    };
    let step = &results.by_mesh[part][contour.step_index];
    for mut text in &mut text {
        text.set_if_neq(Text::new(describe(part, step, &contour.field_name, &hit)));
    }
    let tooltip_position = Vec2::new(
        (cursor.x + 18.0).min((window.width() - 288.0).max(8.0)),
        (cursor.y + 18.0).min((window.height() - 160.0).max(8.0)),
    );
    let marker = camera.world_to_viewport(transform, hit.point).ok();
    for (kind, mut node) in &mut overlays {
        let position = match kind {
            Overlay::Tooltip => Some(tooltip_position),
            Overlay::Marker => marker.map(|p| p - Vec2::splat(4.0)),
        };
        if let Some(position) = position {
            node.display = Display::Flex;
            node.left = px(position.x);
            node.top = px(position.y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn label_distinguishes_nodal_sample_from_element_value_and_missing_values() {
        let mut hit = ProbeHit {
            distance: 1.0,
            point: Vec3::ZERO,
            element: Some(fem_core::ElementId(41)),
            node: Some(fem_core::NodeId(99)),
            value: None,
        };
        let step = fem_core::StepResult {
            step: 7,
            time: 0.5,
            ..default()
        };
        let label = describe(1, &step, "Nodal stress", &hit);
        assert!(label.contains("Node 99 | nearest triangle node"));
        assert!(label.contains("Element 41"));
        assert!(label.contains("unavailable"));
        hit.node = None;
        hit.value = Some(42.0);
        assert!(describe(1, &step, "E", &hit).contains("Element value | no nodal averaging"));
    }
    #[test]
    fn probe_system_initializes_and_hides_outside_results() {
        let mut app = App::new();
        app.init_resource::<SidebarPage>()
            .init_resource::<PlaybackState>()
            .init_resource::<UiPointerState>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ResultGeometry>()
            .init_resource::<FemModelVersion>()
            .init_resource::<FemResultSet>()
            .init_resource::<VisualizationSettings>();
        register(&mut app);
        app.update();
        assert!(
            app.world_mut()
                .query::<(&Overlay, &Node)>()
                .iter(app.world())
                .all(|(_, n)| n.display == Display::None)
        );
    }

    #[test]
    fn hover_uses_result_geometry_and_hides_during_playback_and_ui_interaction() {
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
        use fem_core::{FemMesh, ResultField, StepResult};
        let mut app = App::new();
        app.insert_resource(SidebarPage::Results)
            .init_resource::<PlaybackState>()
            .init_resource::<UiPointerState>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<FemModelVersion>();
        let mut mesh = FemMesh::demo_hex8();
        for node in &mut mesh.nodes {
            node.position += Vec3::X * 50.0;
        }
        let center = mesh.nodes.iter().map(|n| n.position).sum::<Vec3>() / 8.0;
        app.insert_resource(FemModel::demo_hex8()); // hidden pre-model elsewhere
        app.insert_resource(ResultGeometry {
            model: Some(FemModel::single_mesh("Result", mesh)),
            visible: true,
        });
        app.insert_resource(FemResultSet {
            by_mesh: vec![vec![StepResult {
                fields: vec![ResultField::ElementScalar {
                    name: "E".into(),
                    values: vec![42.0],
                    min: 42.0,
                    max: 42.0,
                }],
                ..default()
            }]],
            ..default()
        });
        app.insert_resource(VisualizationSettings {
            contour: Some(visualization::ContourSettings {
                mesh_index: 0,
                step_index: 0,
                field_name: "E".into(),
                show_deformation: false,
                displacement_field: "Displacement".into(),
                deformation_scale: 1.0,
            }),
            ..default()
        });
        let mut window = Window {
            resolution: (800, 600).into(),
            ..default()
        };
        window.set_cursor_position(Some(Vec2::new(400.0, 300.0)));
        app.world_mut().spawn((window, PrimaryWindow));
        app.world_mut().spawn((
            MainViewportCamera,
            Camera {
                computed: ComputedCameraValues {
                    clip_from_view: Mat4::perspective_infinite_reverse_rh(1.0, 800.0 / 600.0, 0.1),
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(800, 600),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                ..default()
            },
            GlobalTransform::from_translation(center + Vec3::Z * 10.0),
        ));
        register(&mut app);
        app.update();
        let tooltip = app
            .world_mut()
            .query::<(Entity, &Overlay)>()
            .iter(app.world())
            .find(|(_, kind)| matches!(kind, Overlay::Tooltip))
            .unwrap()
            .0;
        assert_eq!(
            app.world().get::<Node>(tooltip).unwrap().display,
            Display::Flex
        );
        let label = app
            .world_mut()
            .query_filtered::<&Text, With<ProbeText>>()
            .single(app.world())
            .unwrap();
        assert!(label.0.contains("4.200000e1"));
        app.world_mut().resource_mut::<PlaybackState>().playing = true;
        app.update();
        assert_eq!(
            app.world().get::<Node>(tooltip).unwrap().display,
            Display::None
        );
        app.world_mut().resource_mut::<PlaybackState>().playing = false;
        app.world_mut().resource_mut::<UiPointerState>().over_ui = true;
        app.update();
        assert_eq!(
            app.world().get::<Node>(tooltip).unwrap().display,
            Display::None
        );
        app.world_mut().resource_mut::<UiPointerState>().over_ui = false;
        app.update();
        assert_eq!(
            app.world().get::<Node>(tooltip).unwrap().display,
            Display::Flex
        );
    }
}
