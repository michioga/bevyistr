//! Visualization of boundary conditions and loads: constraint symbols
//! (cones along each restrained translational axis) and load arrows
//! (cylinder shaft + cone head, scaled by relative magnitude).
//!
//! This is the visual half of CLAUDE.md's "human ↔ solver" bridge applied
//! to analysis setup — a `.cnt` file's `!BOUNDARY`/`!CLOAD` blocks are just
//! numbers until you can see where they land on the model.

use bevy::math::primitives::{Cone, Cylinder};
use bevy::mesh::Mesh3d;
use bevy::pbr::MeshMaterial3d;
use bevy::prelude::*;

use fem_core::{AnalysisSetup, FemModel};

use crate::demo_mesh::model_visual_scale;

/// Marker for every entity spawned by [`spawn_boundary_visuals`], so a
/// later reload can despawn exactly this set before respawning.
#[derive(Component)]
pub struct BoundaryVisual;

/// Toggle for whether boundary condition / load symbols are drawn at all.
#[derive(Resource, Debug, Clone, Copy)]
pub struct BoundaryVisualSettings {
    pub show_constraints: bool,
    pub show_loads: bool,
}

impl Default for BoundaryVisualSettings {
    fn default() -> Self {
        Self {
            show_constraints: true,
            show_loads: true,
        }
    }
}

const CONSTRAINT_COLOR: Color = Color::srgb(0.95, 0.30, 0.20);
const LOAD_COLOR: Color = Color::srgb(0.95, 0.65, 0.10);
const PRESSURE_COLOR: Color = Color::srgb(0.35, 0.75, 0.95);
const GRAVITY_COLOR: Color = Color::srgb(0.70, 0.55, 0.95);

/// (Re)spawns constraint cones and load arrows whenever [`AnalysisSetup`]
/// or [`BoundaryVisualSettings`] changes.
///
/// Despawns the previous set first — this mirrors
/// [`crate::demo_mesh::respawn_visuals_on_reload`]'s approach rather than
/// trying to diff individual BCs/loads, since `.cnt` files are typically
/// loaded once per session and the set count is small (tens to low
/// hundreds), so a full rebuild is cheap and far simpler than incremental
/// updates.
pub fn spawn_boundary_visuals(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    setup: Option<Res<AnalysisSetup>>,
    settings: Res<BoundaryVisualSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<BoundaryVisual>>,
) {
    let setup_changed = setup.as_ref().is_some_and(|s| s.is_changed());
    let settings_changed = settings.is_changed();

    if !setup_changed && !settings_changed {
        return;
    }

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let Some(model) = model.as_deref() else { return; };
    let Some(setup) = setup.as_deref() else { return; };

    if setup.is_empty() {
        return;
    }

    let scale = model_visual_scale(model);
    let symbol_size = (scale * 0.02).max(1.0e-3);

    let constraint_material = materials.add(StandardMaterial {
        base_color: CONSTRAINT_COLOR,
        unlit: true,
        ..default()
    });
    let load_material = materials.add(StandardMaterial {
        base_color: LOAD_COLOR,
        unlit: true,
        ..default()
    });
    let pressure_material = materials.add(StandardMaterial {
        base_color: PRESSURE_COLOR,
        unlit: true,
        ..default()
    });
    let gravity_material = materials.add(StandardMaterial {
        base_color: GRAVITY_COLOR,
        unlit: true,
        ..default()
    });

    if settings.show_constraints {
        spawn_constraint_symbols(
            &mut commands, &mut meshes, model, setup, symbol_size, constraint_material,
        );
    }

    if settings.show_loads {
        spawn_load_arrows(
            &mut commands, &mut meshes, model, setup, symbol_size, load_material,
        );
        spawn_dload_arrows(
            &mut commands, &mut meshes, model, setup, symbol_size,
            pressure_material, gravity_material,
        );
    }
}

/// One cone per restrained translational axis (Ux/Uy/Uz), pointing inward
/// toward the node — the conventional FEM "roller/pin" constraint symbol.
/// Rotational-only constraints (Rx-Rz) are skipped: there is no
/// universally recognized lightweight 3-D glyph for a rotational
/// constraint, and adding one would clutter the view without a strong
/// payoff; the dof_label is still available via hover/inspection text.
fn spawn_constraint_symbols(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    model: &FemModel,
    setup: &AnalysisSetup,
    size: f32,
    material: Handle<StandardMaterial>,
) {
    let cone_mesh = meshes.add(Cone { radius: size * 0.5, height: size });

    for bc in &setup.boundary_conditions {
        if !bc.constrains_translation() {
            continue;
        }

        let Some(mesh) = model.meshes.get(bc.mesh_index) else { continue; };

        // Which translational axes (1=X, 2=Y, 3=Z) fall in [dof_start, dof_end].
        let axes: Vec<Vec3> = [(1u8, Vec3::X), (2, Vec3::Y), (3, Vec3::Z)]
            .into_iter()
            .filter(|(dof, _)| *dof >= bc.dof_start && *dof <= bc.dof_end)
            .map(|(_, axis)| axis)
            .collect();

        for &node_id in &bc.nodes {
            let Some(position) = mesh.node_position(node_id) else { continue; };

            for axis in &axes {
                // Cone tip touches the node, base sits one symbol-length
                // below along the constrained axis (pointing "into" the
                // node from outside, like a support symbol).
                let base_center = position - *axis * (size * 0.5);
                let rotation = Quat::from_rotation_arc(Vec3::Y, -*axis);

                commands.spawn((
                    Mesh3d(cone_mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform {
                        translation: base_center,
                        rotation,
                        ..default()
                    },
                    BoundaryVisual,
                    Name::new(format!("BC {} @ node {}", bc.name, node_id.0)),
                ));
            }
        }
    }
}

/// One arrow (cylinder shaft + cone head) per nodal load, scaled by its
/// magnitude relative to the largest load in the set so the visualization
/// communicates *relative* load intensity at a glance.
fn spawn_load_arrows(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    model: &FemModel,
    setup: &AnalysisSetup,
    base_size: f32,
    material: Handle<StandardMaterial>,
) {
    let max_magnitude = setup
        .nodal_loads
        .iter()
        .map(|load| load.value.abs())
        .fold(0.0f32, f32::max)
        .max(1.0e-9);

    for load in &setup.nodal_loads {
        let Some(mesh) = model.meshes.get(load.mesh_index) else { continue; };
        let Some(position) = mesh.node_position(load.node) else { continue; };

        let axis = match load.dof {
            1 => Vec3::X,
            2 => Vec3::Y,
            3 => Vec3::Z,
            _ => continue, // moments (DOF 4-6) not visualized as arrows
        };

        let direction = axis * load.value.signum();
        // Arrow length scales between 1x and 3x the base symbol size by
        // relative magnitude, so a small load doesn't visually disappear
        // and a dominant load doesn't dwarf the model.
        let length = base_size * (1.0 + 2.0 * (load.value.abs() / max_magnitude));

        let shaft_len = length * 0.7;
        let head_len = length * 0.3;
        let radius = base_size * 0.12;

        let rotation = Quat::from_rotation_arc(Vec3::Y, direction);

        // Shaft: cylinder from the node outward.
        let shaft_mesh = meshes.add(Cylinder { radius: radius * 0.6, half_height: shaft_len * 0.5 });
        let shaft_center = position + direction * (shaft_len * 0.5);

        commands.spawn((
            Mesh3d(shaft_mesh),
            MeshMaterial3d(material.clone()),
            Transform { translation: shaft_center, rotation, ..default() },
            BoundaryVisual,
            Name::new(format!("Load shaft {} @ node {}", load.name, load.node.0)),
        ));

        // Head: cone at the tip, pointing in the load direction.
        let head_mesh = meshes.add(Cone { radius, height: head_len });
        let head_center = position + direction * (shaft_len + head_len * 0.5);

        commands.spawn((
            Mesh3d(head_mesh),
            MeshMaterial3d(material.clone()),
            Transform { translation: head_center, rotation, ..default() },
            BoundaryVisual,
            Name::new(format!("Load head {} @ node {}", load.name, load.node.0)),
        ));
    }
}

/// One arrow per pressure [`fem_core::DistributedLoad`] face, drawn from
/// the face centroid along its outward normal (reversed for a negative
/// magnitude, matching the FrontISTR/Abaqus convention that positive
/// pressure acts *into* the surface) — the surface-load counterpart of
/// [`spawn_load_arrows`]'s nodal-load arrows.
///
/// Gravity loads don't carry direction data (see
/// [`fem_core::DistributedLoadKind::Gravity`]'s doc comment — only a
/// magnitude is stored, direction is implicit), so they get one schematic
/// arrow pointing -Y from the centroid of their targeted elements instead
/// of a precise per-face rendering — enough to confirm "a gravity load
/// exists and roughly where," which is the only claim that can honestly be
/// drawn from the data available.
fn spawn_dload_arrows(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    model: &FemModel,
    setup: &AnalysisSetup,
    base_size: f32,
    pressure_material: Handle<StandardMaterial>,
    gravity_material: Handle<StandardMaterial>,
) {
    let max_magnitude = setup
        .distributed_loads
        .iter()
        .map(|dl| dl.value.abs())
        .fold(0.0f32, f32::max)
        .max(1.0e-9);

    // Element-face → geometry lookups, built lazily per mesh (most models
    // won't have distributed loads on every mesh in an assembly).
    let mut face_lookup: std::collections::HashMap<
        usize,
        std::collections::HashMap<fem_core::ElementFaceRef, fem_core::FaceGeometry>,
    > = std::collections::HashMap::new();

    for dl in &setup.distributed_loads {
        let Some(mesh) = model.meshes.get(dl.mesh_index) else { continue; };

        let length = base_size * (1.0 + 2.0 * (dl.value.abs() / max_magnitude));
        let shaft_len = length * 0.7;
        let head_len = length * 0.3;
        let radius = base_size * 0.10;

        match (dl.kind, &dl.target) {
            (fem_core::DistributedLoadKind::Pressure, fem_core::DistributedLoadTarget::Faces(faces)) => {
                let lookup = face_lookup.entry(dl.mesh_index).or_insert_with(|| {
                    mesh.cached_boundary_faces()
                        .iter()
                        .filter_map(|face| {
                            let face_ref = face.element_face_ref()?;
                            let geom = mesh.face_geometry(face)?;
                            Some((face_ref, geom))
                        })
                        .collect()
                });

                for face_ref in faces {
                    let Some(geom) = lookup.get(face_ref) else { continue; };

                    let direction = -geom.normal * dl.value.signum();
                    let rotation = Quat::from_rotation_arc(Vec3::Y, direction);

                    let shaft_mesh = meshes.add(Cylinder { radius: radius * 0.6, half_height: shaft_len * 0.5 });
                    let shaft_center = geom.centroid + direction * (shaft_len * 0.5);

                    commands.spawn((
                        Mesh3d(shaft_mesh),
                        MeshMaterial3d(pressure_material.clone()),
                        Transform { translation: shaft_center, rotation, ..default() },
                        BoundaryVisual,
                        Name::new(format!("DLoad shaft {} @ elem {}", dl.name, face_ref.element.0)),
                    ));

                    let head_mesh = meshes.add(Cone { radius, height: head_len });
                    let head_center = geom.centroid + direction * (shaft_len + head_len * 0.5);

                    commands.spawn((
                        Mesh3d(head_mesh),
                        MeshMaterial3d(pressure_material.clone()),
                        Transform { translation: head_center, rotation, ..default() },
                        BoundaryVisual,
                        Name::new(format!("DLoad head {} @ elem {}", dl.name, face_ref.element.0)),
                    ));
                }
            }
            (fem_core::DistributedLoadKind::Gravity, target) => {
                let elements: std::collections::HashSet<fem_core::ElementId> =
                    target.element_ids().into_iter().collect();

                if elements.is_empty() {
                    continue;
                }

                let mut centroid = Vec3::ZERO;
                let mut count = 0u32;

                for element in &mesh.elements {
                    if !elements.contains(&element.id) {
                        continue;
                    }
                    if let Some(positions) = mesh.node_positions(&element.nodes) {
                        for p in positions {
                            centroid += p;
                            count += 1;
                        }
                    }
                }

                if count == 0 {
                    continue;
                }
                centroid /= count as f32;

                let direction = Vec3::NEG_Y;
                let rotation = Quat::from_rotation_arc(Vec3::Y, direction);

                let shaft_mesh = meshes.add(Cylinder { radius: radius * 0.6, half_height: shaft_len * 0.5 });
                commands.spawn((
                    Mesh3d(shaft_mesh),
                    MeshMaterial3d(gravity_material.clone()),
                    Transform {
                        translation: centroid + direction * (shaft_len * 0.5),
                        rotation,
                        ..default()
                    },
                    BoundaryVisual,
                    Name::new(format!("DLoad(gravity) shaft {}", dl.name)),
                ));

                let head_mesh = meshes.add(Cone { radius, height: head_len });
                commands.spawn((
                    Mesh3d(head_mesh),
                    MeshMaterial3d(gravity_material.clone()),
                    Transform {
                        translation: centroid + direction * (shaft_len + head_len * 0.5),
                        rotation,
                        ..default()
                    },
                    BoundaryVisual,
                    Name::new(format!("DLoad(gravity) head {}", dl.name)),
                ));
            }
            // A pressure load stored with `DistributedLoadTarget::Elements`
            // (no face info — e.g. hand-built from a bare element group
            // rather than a picked surface) has nothing to anchor an arrow
            // to; skip it rather than guessing a face.
            (fem_core::DistributedLoadKind::Pressure, fem_core::DistributedLoadTarget::Elements(_)) => {}
        }
    }
}
