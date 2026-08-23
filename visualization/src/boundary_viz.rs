//! Visualization of boundary conditions and loads: constraint symbols
//! (cones along each restrained translational axis) and load arrows
//! (cylinder shaft + cone head, scaled by relative magnitude).
//!
//! This is the visual half of CLAUDE.md's "human ↔ solver" bridge applied
//! to analysis setup — a `.cnt` file's `!BOUNDARY`/`!CLOAD` blocks are just
//! numbers until you can see where they land on the model.

use bevy::asset::RenderAssetUsages;
use bevy::math::primitives::{Cone, Cylinder};
use bevy::mesh::{Mesh3d, PrimitiveTopology};
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

    let symbol_size = boundary_symbol_size(model);

    let constraint_material = materials.add(StandardMaterial {
        base_color: CONSTRAINT_COLOR,
        unlit: true,
        cull_mode: None,
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
    let Some(mesh) = build_constraint_mesh(model, setup, size) else {
        return;
    };

    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        Transform::default(),
        BoundaryVisual,
        Name::new("Boundary constraint symbols"),
    ));
}

/// Builds every translational constraint marker into one low-poly mesh.
///
/// A large FrontISTR node group can contain several thousand nodes. Spawning
/// one Bevy entity per node and constrained axis made opening `conrod` create
/// more than 21,000 entities just for the red cones. One triangle mesh keeps
/// the same complete visual coverage while reducing that to a single entity.
fn build_constraint_mesh(model: &FemModel, setup: &AnalysisSetup, size: f32) -> Option<Mesh> {
    const CONE_SIDES: usize = 6;

    let estimated_symbols: usize = setup
        .boundary_conditions
        .iter()
        .map(|bc| {
            let axis_count = (1u8..=3)
                .filter(|dof| *dof >= bc.dof_start && *dof <= bc.dof_end)
                .count();
            bc.nodes.len() * axis_count
        })
        .sum();

    let vertices_per_symbol = CONE_SIDES * 6;
    let mut positions = Vec::with_capacity(estimated_symbols * vertices_per_symbol);
    let mut normals = Vec::with_capacity(estimated_symbols * vertices_per_symbol);

    for bc in &setup.boundary_conditions {
        if !bc.constrains_translation() {
            continue;
        }

        let Some(mesh) = model.meshes.get(bc.mesh_index) else { continue; };

        for &node_id in &bc.nodes {
            let Some(position) = mesh.node_position(node_id) else { continue; };

            for (dof, axis) in [(1u8, Vec3::X), (2, Vec3::Y), (3, Vec3::Z)] {
                if dof < bc.dof_start || dof > bc.dof_end {
                    continue;
                }

                append_constraint_cone(
                    &mut positions,
                    &mut normals,
                    position,
                    axis,
                    size,
                    CONE_SIDES,
                );
            }
        }
    }

    (!positions.is_empty()).then(|| {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    })
}

fn append_constraint_cone(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    node_position: Vec3,
    axis: Vec3,
    size: f32,
    sides: usize,
) {
    let direction = axis.normalize();
    let helper = if direction.dot(Vec3::Y).abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let tangent = direction.cross(helper).normalize();
    let bitangent = direction.cross(tangent).normalize();
    // Match Bevy's original midpoint-anchored Cone transform: the circular
    // base is anchored exactly at the constrained node and the tip extends
    // one symbol length in the negative constrained-axis direction.
    let base_center = node_position;
    let tip = node_position - direction * size;
    let radius = size * 0.5;

    for side in 0..sides {
        let angle0 = std::f32::consts::TAU * side as f32 / sides as f32;
        let angle1 = std::f32::consts::TAU * (side + 1) as f32 / sides as f32;
        let ring0 = base_center + radius * (tangent * angle0.cos() + bitangent * angle0.sin());
        let ring1 = base_center + radius * (tangent * angle1.cos() + bitangent * angle1.sin());

        append_triangle(positions, normals, tip, ring0, ring1);
        append_triangle(positions, normals, base_center, ring1, ring0);
    }
}

fn append_triangle(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    a: Vec3,
    b: Vec3,
    c: Vec3,
) {
    let normal = (b - a).cross(c - a).normalize_or_zero().to_array();

    positions.extend([a.to_array(), b.to_array(), c.to_array()]);
    normals.extend([normal; 3]);
}

/// Chooses a glyph size from the local mesh resolution instead of only the
/// model's overall diagonal. Long, finely meshed parts such as `conrod`
/// otherwise receive cones several edge lengths wide, which overlap into a
/// displaced-looking solid annulus.
fn boundary_symbol_size(model: &FemModel) -> f32 {
    let model_scale = model_visual_scale(model);
    let mut edge_lengths: Vec<f32> = model
        .meshes
        .iter()
        .flat_map(|mesh| {
            mesh.cached_boundary_edges().iter().filter_map(|edge| {
                let a = mesh.node_position(edge.nodes[0])?;
                let b = mesh.node_position(edge.nodes[1])?;
                let length = a.distance(b);
                (length.is_finite() && length > 1.0e-6).then_some(length)
            })
        })
        .collect();

    if edge_lengths.is_empty() {
        return (model_scale * 0.02).max(1.0e-3);
    }

    let middle = edge_lengths.len() / 2;
    let (_, median, _) = edge_lengths.select_nth_unstable_by(middle, f32::total_cmp);

    (*median * 0.45)
        .clamp(model_scale * 0.002, model_scale * 0.02)
        .max(1.0e-3)
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

#[cfg(test)]
mod tests {
    use super::*;
    use fem_core::{BoundaryCondition, FemMesh, FemNode, NodeId};

    #[test]
    fn constraint_markers_are_combined_into_one_mesh() {
        let mesh = FemMesh::new(
            vec![
                FemNode::new(NodeId(1), Vec3::ZERO),
                FemNode::new(NodeId(2), Vec3::ONE),
            ],
            Vec::new(),
        );
        let model = FemModel::single_mesh("test", mesh);
        let mut setup = AnalysisSetup::default();
        setup.boundary_conditions.push(BoundaryCondition {
            name: "FIX".to_string(),
            mesh_index: 0,
            nodes: vec![NodeId(1), NodeId(2)],
            ngrp_name: Some("FIX".to_string()),
            dof_start: 1,
            dof_end: 3,
            value: 0.0,
        });

        let mesh = build_constraint_mesh(&model, &setup, 0.1).unwrap();

        // 2 nodes * 3 axes * 6 cone sides * (side + base) * 3 vertices.
        assert_eq!(mesh.count_vertices(), 2 * 3 * 6 * 2 * 3);
    }

    #[test]
    fn constraint_cone_base_is_anchored_at_the_node() {
        let mut positions = Vec::new();
        let mut normals = Vec::new();

        append_constraint_cone(
            &mut positions,
            &mut normals,
            Vec3::ZERO,
            Vec3::X,
            2.0,
            4,
        );

        // The first side triangle is [tip, base ring 0, base ring 1].
        assert_eq!(positions[0][0], -2.0);
        assert_eq!(positions[1][0], 0.0);
        assert_eq!(positions[2][0], 0.0);
    }

}
