use bevy::dev_tools::infinite_grid::{InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings};
use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::pbr::wireframe::{WireframeConfig, WireframePlugin};
use bevy::prelude::*;
use std::path::PathBuf;

use box_select::BoxSelectPlugin;
use camera::{fit_bounds, radius_limits, CameraPlugin, OrbitCamera};
use fem_core::FemModel;
use interaction::InteractionPlugin;
use picking::PickingPlugin;
use selection::SelectionPlugin;
use ui::UiPlugin;
use visualization::VisualizationPlugin;
use gmsh;

fn main() {
    let arg_path = std::env::args_os().nth(1).map(PathBuf::from);

    // The path can point directly at a mesh file, or at a FrontISTR
    // project's `hecmw_ctrl.dat` — the same two entry points the GUI
    // offers via "Open Mesh" and "Open Project" respectively, so
    // `bevyistr path/to/hecmw_ctrl.dat` opens the mesh *and* its
    // boundary conditions/loads/materials in one shot, matching what
    // running this from a shell should mean for a FrontISTR project
    // directory. Unlike the GUI's "Open Project" button (which requests
    // the mesh asynchronously via `MeshLoadRequest` and has to defer
    // loading the `.cnt` until that finishes — see `PendingCntLoad`'s doc
    // comment), this runs before `App::new()` even exists, so there's no
    // async mesh load to race: both files are read synchronously, in order.
    let mut initial_setup = fem_core::AnalysisSetup::default();
    let mut setup_has_content = false;

    let model = arg_path.and_then(|path| {
        let (mesh_path, cnt_path) = match hecmw::load_hecmw_ctrl(&path) {
            // Only treat this as a project file if it actually parsed a
            // `!MESH` directive — a plain `.msh` file happens to share
            // HECMW's `!KEYWORD` block syntax (`!NODE`, `!ELEMENT`, ...)
            // but none of those match `hecmw_ctrl.dat`'s specific
            // `!MESH`/`!CONTROL` directives, so this falls through to the
            // "just a mesh path" branch correctly rather than resolving to
            // `(None, None)` and silently failing to load anything.
            Ok(content) if content.mesh_path.is_some() => hecmw::resolve_paths(&path, &content),
            _ => (Some(path), None),
        };

        let (model, materials, sections) = load_initial_model(mesh_path?)?;

        for m in materials { initial_setup.materials.push(m); setup_has_content = true; }
        for s in sections  { initial_setup.sections.push(s);  setup_has_content = true; }

        if let Some(cnt_path) = cnt_path {
            match hecmw::load_cnt_file(&cnt_path, &model.meshes[0], 0) {
                Ok(data) => {
                    initial_setup.boundary_conditions.extend(data.boundary_conditions);
                    initial_setup.nodal_loads.extend(data.nodal_loads);
                    initial_setup.distributed_loads.extend(data.distributed_loads);
                    initial_setup.materials.extend(data.materials);
                    initial_setup.sections.extend(data.sections);
                    setup_has_content = true;
                }
                Err(e) => eprintln!("Failed to parse {}: {e}", cnt_path.display()),
            }
        }

        Some(model)
    });

    let mut app = App::new();

    if let Some(model) = model {
        app.insert_resource(model);

        if setup_has_content {
            app.insert_resource(initial_setup);
        }
    }

    app.insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.025)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.82, 0.88, 0.95),
            brightness: 350.0,
            ..default()
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "bevyistr — FrontISTR Pre/Post".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(WireframePlugin::default())
        .insert_resource(WireframeConfig {
            global: false,
            default_color: Color::srgb(0.55, 0.82, 0.95),
            ..default()
        })
        // ── FEM-specific plugins ──────────────────────────────────────
        .add_plugins((
            CameraPlugin,
            InteractionPlugin,
            PickingPlugin,
            SelectionPlugin,
            VisualizationPlugin,
            UiPlugin,
            BoxSelectPlugin,
        ))
        // ── Bevy 0.19 new features ────────────────────────────────────
        .add_plugins(FrameTimeDiagnosticsPlugin { ..default() })
        // InfiniteGridPlugin: reference grid in the 3-D viewport.
        // Provides immediate spatial/scale context when working on FEM
        // models — every pre/post tool has one, and Bevy 0.19 ships it
        // as a first-class plugin rather than requiring a third-party crate.
        .add_plugins(InfiniteGridPlugin)
        .add_systems(Startup, (setup, spawn_grid))
        .add_systems(Update, rescale_grid_on_reload)
        .run();
}

/// Loads the initial mesh from the command-line argument (if given).
/// Uses the extended loader that also extracts embedded !MATERIAL/!SECTION
/// blocks from the .msh file, so `hinge.msh` sets up steel + solid section
/// automatically.
fn load_initial_model(path: PathBuf)
    -> Option<(FemModel, Vec<fem_core::FemMaterial>, Vec<fem_core::Section>)>
{
    match hecmw::load_mesh_file_with_setup(&path) {
        Ok((mesh, materials, sections)) => {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("mesh")
                .to_string();
            Some((FemModel::single_mesh(name, mesh), materials, sections))
        }
        Err(e) => {
            // HECMW parse failed — try Gmsh MSH v4.1 fallback
            match gmsh::load_msh_file(&path) {
                Ok(mesh) => {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("mesh")
                        .to_string();
                    Some((FemModel::single_mesh(name, mesh), Vec::new(), Vec::new()))
                }
                Err(_) => {
                    eprintln!("Failed to load {}: {e}", path.display());
                    None
                }
            }
        }
    }
}

fn setup(mut commands: Commands, model: Option<Res<FemModel>>) {
    let (focus, radius) = model
        .as_deref()
        .and_then(FemModel::bounds)
        .map(|(min, max)| fit_bounds(min, max))
        .unwrap_or((Vec3::ZERO, 10.0));
    let (min_radius, max_radius) = radius_limits(radius);
    let camera_position = focus + Vec3::new(radius * 0.45, radius * 0.45, radius);

    commands.spawn((
        Camera3d::default(),
        Camera::default(),
        Transform::from_translation(camera_position).looking_at(focus, Vec3::Y),
        OrbitCamera {
            focus,
            target_focus: focus,
            radius,
            min_radius,
            max_radius,
        },
    ));

    // Key light
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(1.0, 0.96, 0.90),
            illuminance: 3_200.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(6.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Fill light
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.78, 0.86, 0.95),
            illuminance: 1_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-5.0, -3.0, -5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Spawns the infinite reference grid in the XZ plane.
///
/// This is a Bevy 0.19 first-class feature (`InfiniteGridPlugin` +
/// `InfiniteGrid` component). For a FEM viewer the grid serves as an
/// immediate visual frame of reference — model scale, orientation, and
/// rough dimensions are all readable at a glance without needing to measure
/// bounding-box numbers.
///
/// The grid is rendered at Y=0. When a model sits above or below that
/// level, the camera still orbits around the model's centroid, so the
/// grid recedes naturally into the background as the model dominates
/// the view.
fn spawn_grid(mut commands: Commands) {
    commands.spawn((
        InfiniteGrid,
        InfiniteGridSettings {
            x_axis_color:   Color::srgb(0.70, 0.20, 0.20),
            z_axis_color:   Color::srgb(0.20, 0.50, 0.70),
            major_line_color: Color::srgba(0.35, 0.50, 0.60, 0.55),
            minor_line_color: Color::srgba(0.20, 0.32, 0.40, 0.30),
            ..default()
        },
    ));
}

/// Re-tunes the grid's line spacing (`scale`) and camera-relative fade
/// distance (`fadeout_distance`) to the loaded model's size, whenever the
/// model changes.
///
/// `InfiniteGridSettings`'s defaults (`fadeout_distance: 100.0`, `scale:
/// 1.0`) are tuned for a roughly meter-scale scene sitting near the
/// origin. FEM meshes are very often authored in millimetres, though, and
/// a mechanical part's bounding box can easily span a couple hundred raw
/// units — well past the fixed 100-unit fade distance the camera sits
/// beyond once framed via [`fit_bounds`]. The grid isn't just "coarser"
/// in that case, it's rendered fully transparent across the entire
/// visible area, i.e. it appears to vanish outright. Scaling both values
/// to the model's own framing radius (calibrated so a model at the demo
/// cube's scale reproduces today's defaults exactly) keeps the grid
/// visible — and its line spacing sensible — regardless of what units
/// the mesh happens to use.
fn rescale_grid_on_reload(
    model:            Option<Res<FemModel>>,
    version:          Res<fem_core::FemModelVersion>,
    mut last_version: Local<Option<u64>>,
    mut grid_query:   Query<&mut InfiniteGridSettings, With<InfiniteGrid>>,
) {
    let current = version.value;

    if *last_version == Some(current) {
        return;
    }
    *last_version = Some(current);

    let Some((min, max)) = model.as_deref().and_then(FemModel::bounds) else { return; };
    let (_, radius) = fit_bounds(min, max);

    let Ok(mut settings) = grid_query.single_mut() else { return; };

    // Calibrated against the built-in demo cube's own fit radius (~3.3
    // units), where the untouched defaults already look right — a model
    // at that scale should end up with ~today's fadeout_distance/scale,
    // not a discontinuity right at the calibration point.
    const REFERENCE_RADIUS: f32 = 3.3;

    settings.fadeout_distance = (radius * 6.0).max(100.0);
    settings.scale = (REFERENCE_RADIUS / radius).clamp(1.0e-4, 10.0);
}
