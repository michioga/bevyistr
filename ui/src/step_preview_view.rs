use crate::{layout::SidebarPage, project_io::fit_orbit_camera, step_import::StepImport};
use bevy::{camera::visibility::RenderLayers, prelude::*, window::PrimaryWindow};
use camera::OrbitCamera;
use fem_core::{MainViewportCamera, UiPointerState};

const STEP_PREVIEW_LAYER: usize = 31;

#[derive(Component)]
pub(crate) struct StepPreviewVisual;

#[derive(Component)]
pub(crate) struct StepContactHighlight;

struct SavedStepView {
    transform: Transform,
    orbit: OrbitCamera,
    layers: Option<RenderLayers>,
}

#[derive(Default)]
pub(crate) struct StepPreviewView {
    original: Option<SavedStepView>,
    drawn: Option<u64>,
}

pub(crate) fn draw_step_preview(
    mut commands: Commands,
    mut step: ResMut<StepImport>,
    page: Res<SidebarPage>,
    mut view: Local<StepPreviewView>,
    mut camera: Query<
        (
            Entity,
            &mut Transform,
            &mut OrbitCamera,
            Option<&RenderLayers>,
        ),
        With<MainViewportCamera>,
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    visuals: Query<Entity, With<StepPreviewVisual>>,
) {
    let active = step.preview();
    let revision = active.map(|(revision, _)| revision);
    if view.drawn != revision {
        for visual in &visuals {
            commands.entity(visual).despawn();
        }
        view.drawn = None;
    }

    let Ok((camera, mut transform, mut orbit, layers)) = camera.single_mut() else {
        return;
    };
    let Some((revision, bounds)) = active.filter(|_| *page == SidebarPage::Model) else {
        if let Some(original) = view.original.take() {
            *transform = original.transform;
            *orbit = original.orbit;
            if let Some(layers) = original.layers {
                commands.entity(camera).insert(layers);
            } else {
                commands.entity(camera).remove::<RenderLayers>();
            }
        }
        return;
    };

    if view.original.is_none() {
        view.original = Some(SavedStepView {
            transform: *transform,
            orbit: orbit.clone(),
            layers: layers.cloned(),
        });
        commands
            .entity(camera)
            .insert(RenderLayers::layer(STEP_PREVIEW_LAYER));
        if let Some((min, max)) = bounds {
            fit_orbit_camera(&mut transform, &mut orbit, min, max);
        }
    }
    if view.drawn == Some(revision) {
        return;
    }
    let Some(preview) = step.take_preview_meshes() else {
        return;
    };
    view.drawn = Some(revision);
    let layer = RenderLayers::layer(STEP_PREVIEW_LAYER);
    if let Some(surface) = preview.surface {
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.30, 0.60, 0.67),
            perceptual_roughness: 0.82,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        commands.spawn((
            StepPreviewVisual,
            layer.clone(),
            Mesh3d(meshes.add(surface)),
            MeshMaterial3d(material),
            Transform::default(),
            Name::new("STEP preview surface"),
        ));
    }
    if let Some(edges) = preview.edges {
        // Preview edges are a LineList. WGPU only permits depth bias on triangles.
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.035, 0.07, 0.10),
            unlit: true,
            ..default()
        });
        commands.spawn((
            StepPreviewVisual,
            layer.clone(),
            Mesh3d(meshes.add(edges)),
            MeshMaterial3d(material),
            Transform::default(),
            Name::new("STEP preview edges"),
        ));
    }
    commands.spawn((
        StepPreviewVisual,
        layer,
        DirectionalLight {
            illuminance: 4200.0,
            ..default()
        },
        Transform::from_xyz(2.0, 4.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
        Name::new("STEP preview light"),
    ));
}

pub(crate) fn pick_step_contact_face(
    page: Res<SidebarPage>,
    mut step: ResMut<StepImport>,
    buttons: Res<ButtonInput<MouseButton>>,
    pointer: Res<UiPointerState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<MainViewportCamera>>,
) {
    if *page != SidebarPage::Model
        || step.preview().is_none()
        || pointer.over_ui
        || pointer.navigating
        || !buttons.just_pressed(MouseButton::Left)
    {
        return;
    }
    let Some(cursor) = windows.single().ok().and_then(Window::cursor_position) else {
        return;
    };
    let Ok((camera, transform)) = camera.single() else {
        return;
    };
    if let Ok(ray) = camera.viewport_to_world(transform, cursor) {
        step.toggle_contact_face(ray.origin, *ray.direction);
    }
}

pub(crate) fn update_step_contact_highlight(
    mut commands: Commands,
    step: Res<StepImport>,
    page: Res<SidebarPage>,
    mut drawn: Local<Option<(u64, u64)>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    highlights: Query<Entity, With<StepContactHighlight>>,
) {
    let revision = step
        .contact_highlight_revision()
        .filter(|_| *page == SidebarPage::Model);
    if *drawn == revision {
        return;
    }
    *drawn = revision;
    for highlight in &highlights {
        commands.entity(highlight).despawn();
    }
    if revision.is_none() {
        return;
    }
    let Some(surface) = step.build_contact_highlight() else {
        return;
    };
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.61, 0.16, 0.55),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        depth_bias: 1.0,
        ..default()
    });
    commands.spawn((
        StepContactHighlight,
        RenderLayers::layer(STEP_PREVIEW_LAYER),
        Mesh3d(meshes.add(surface)),
        MeshMaterial3d(material),
        Name::new("STEP contact refinement"),
    ));
}
