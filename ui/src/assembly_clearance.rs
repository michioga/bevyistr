//! On-demand assembly clearance checks backed by Avian's manual contact queries.
//!
//! This deliberately does not install Avian's physics plugins.  FEM picking,
//! contact generation, and part transforms remain owned by bevyistr; Avian is
//! used only as a robust geometric distance/intersection utility when the user
//! explicitly asks for a check.

use crate::assembly::AssemblyEditorState;
use crate::layout::SidebarPage;
use avian3d::collision::collider::contact_query::{
    ClosestPoints, closest_points, contact, distance, intersection_test,
};
use avian3d::prelude::{Collider, TrimeshFlags};
use bevy::prelude::*;
use fem_core::{FemMesh, FemModel, FemModelVersion, NodeId};
use std::collections::BTreeMap;

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const TEXT_OK: Color = Color::srgb(0.45, 0.88, 0.68);
const TEXT_WARNING: Color = Color::srgb(1.0, 0.72, 0.28);
const TEXT_ERROR: Color = Color::srgb(1.0, 0.38, 0.34);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);

#[derive(Component)]
pub(crate) struct AssemblyClearanceButton;

#[derive(Component)]
pub(crate) struct AssemblyClearanceText;

/// Separate gizmo styling keeps the clearance measurement legible without
/// changing the grid and other viewport guides that use Bevy's default group.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(crate) struct AssemblyClearanceGizmos;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClearanceKind {
    Separated,
    Touching,
    Intersecting,
}

#[derive(Debug, Clone)]
struct ClearanceReport {
    checked_version: u64,
    selected_part: usize,
    other_part: usize,
    kind: ClearanceKind,
    distance: f32,
    affected_parts: usize,
    closest_points: Option<(Vec3, Vec3)>,
}

#[derive(Clone, Debug)]
struct PartCollider {
    shape: Collider,
    interior_sample: Option<Vec3>,
}

#[derive(Resource)]
pub(crate) struct AssemblyClearanceState {
    collider_version: Option<u64>,
    colliders: Vec<Result<PartCollider, String>>,
    report: Option<ClearanceReport>,
    message: String,
    tone: ClearanceTone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ClearanceTone {
    #[default]
    Muted,
    Ok,
    Warning,
    Error,
}

impl Default for AssemblyClearanceState {
    fn default() -> Self {
        Self {
            collider_version: None,
            colliders: Vec::new(),
            report: None,
            message: "Select a part, then check clearance".to_string(),
            tone: ClearanceTone::Muted,
        }
    }
}

impl AssemblyClearanceState {
    fn rebuild_colliders(&mut self, model: &FemModel, version: u64) {
        if self.collider_version == Some(version) && self.colliders.len() == model.parts.len() {
            return;
        }

        self.colliders = model
            .parts
            .iter()
            .map(|part| {
                model
                    .meshes
                    .get(part.mesh_index)
                    .ok_or_else(|| format!("mesh {} is missing", part.mesh_index + 1))
                    .and_then(build_boundary_collider)
            })
            .collect();
        self.collider_version = Some(version);
    }

    fn check(&mut self, model: &FemModel, version: u64, selected_part: Option<usize>) {
        self.report = None;

        let Some(selected_part) = selected_part else {
            self.set_message(
                "Select a part before checking clearance",
                ClearanceTone::Warning,
            );
            return;
        };
        if model.parts.len() < 2 {
            self.set_message(
                "Add at least two parts to check clearance",
                ClearanceTone::Muted,
            );
            return;
        }
        if selected_part >= model.parts.len() {
            self.set_message(
                "The selected part is no longer available",
                ClearanceTone::Error,
            );
            return;
        }

        self.rebuild_colliders(model, version);
        let report = match evaluate_selected_part(model, &self.colliders, selected_part, version) {
            Ok(report) => report,
            Err(error) => {
                self.set_message(error, ClearanceTone::Error);
                return;
            }
        };

        let selected_name = &model.parts[report.selected_part].name;
        let other_name = &model.parts[report.other_part].name;
        let suffix = if report.affected_parts > 1 {
            format!(" (+{} more)", report.affected_parts - 1)
        } else {
            String::new()
        };
        let gap_vector = report
            .closest_points
            .map(|(selected_point, other_point)| other_point - selected_point);
        match report.kind {
            ClearanceKind::Separated => self.set_message(
                if let Some(vector) = gap_vector {
                    format!(
                        "Clearance: {:.6} model units\n{} -> [{}] {}\nGap vector: ({:.6}, {:.6}, {:.6})",
                        report.distance,
                        selected_name,
                        report.other_part + 1,
                        other_name,
                        vector.x,
                        vector.y,
                        vector.z
                    )
                } else {
                    format!(
                        "Clearance: {:.6} model units\n{} -> [{}] {}",
                        report.distance,
                        selected_name,
                        report.other_part + 1,
                        other_name
                    )
                },
                ClearanceTone::Ok,
            ),
            ClearanceKind::Touching => self.set_message(
                format!(
                    "TOUCHING (within tolerance)\n{} -> [{}] {}{}",
                    selected_name,
                    report.other_part + 1,
                    other_name,
                    suffix
                ),
                ClearanceTone::Warning,
            ),
            ClearanceKind::Intersecting => self.set_message(
                format!(
                    "INTERFERENCE DETECTED\n{} -> [{}] {}{}",
                    selected_name,
                    report.other_part + 1,
                    other_name,
                    suffix
                ),
                ClearanceTone::Error,
            ),
        }
        self.report = Some(report);
    }

    fn set_message(&mut self, message: impl Into<String>, tone: ClearanceTone) {
        self.message = message.into();
        self.tone = tone;
    }
}

pub(crate) fn spawn_assembly_clearance_ui(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Button,
            Node {
                width: percent(100.0),
                height: px(29.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(5.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            AssemblyClearanceButton,
            Name::new("AssemblyClearanceButton"),
        ))
        .with_child((
            Text::new("Check clearance to other parts"),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(TEXT_MAIN),
        ));

    parent.spawn((
        Text::new("Select a part, then check clearance"),
        TextFont {
            font_size: FontSize::Px(10.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
        AssemblyClearanceText,
        Name::new("AssemblyClearanceText"),
    ));
    parent.spawn((
        Text::new("Read-only query; nearest points appear in the viewport"),
        TextFont {
            font_size: FontSize::Px(9.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
    ));
}

/// Draws the most recent Avian query as a viewport measurement.  It is
/// intentionally immediate-mode: changing any mesh version makes the report
/// stale and removes the preview until the user checks again.
pub(crate) fn draw_assembly_clearance_preview(
    model: Res<FemModel>,
    version: Res<FemModelVersion>,
    page: Res<SidebarPage>,
    clearance: Res<AssemblyClearanceState>,
    mut gizmos: Gizmos<AssemblyClearanceGizmos>,
) {
    if !matches!(*page, SidebarPage::Model | SidebarPage::Contact) {
        return;
    }

    let Some(report) = clearance
        .report
        .as_ref()
        .filter(|report| report.checked_version == version.value)
    else {
        return;
    };
    let Some((selected_point, other_point)) = report.closest_points else {
        return;
    };
    if !selected_point.is_finite() || !other_point.is_finite() {
        return;
    }

    let color = match report.kind {
        ClearanceKind::Separated => Color::srgb(0.20, 0.88, 1.0),
        ClearanceKind::Touching => Color::srgb(1.0, 0.72, 0.18),
        ClearanceKind::Intersecting => Color::srgb(1.0, 0.22, 0.18),
    };
    let marker_radius = model
        .bounds()
        .map(|(min, max)| min.distance(max) * 0.004)
        .filter(|radius| radius.is_finite() && *radius > 1.0e-7)
        .unwrap_or(0.01);

    gizmos.sphere(selected_point, marker_radius, color);
    if selected_point.distance_squared(other_point) > marker_radius * marker_radius * 0.01 {
        gizmos.line(selected_point, other_point, color);
        gizmos.sphere(other_point, marker_radius, color);
    }
}

pub(crate) fn assembly_clearance_button_system(
    model: Res<FemModel>,
    version: Res<FemModelVersion>,
    editor: Res<AssemblyEditorState>,
    mut clearance: ResMut<AssemblyClearanceState>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<AssemblyClearanceButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            clearance.check(&model, version.value, editor.selected_part);
        }

        *background = BackgroundColor(match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        });
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn update_assembly_clearance_text(
    version: Res<FemModelVersion>,
    clearance: Res<AssemblyClearanceState>,
    mut texts: Query<(&mut Text, &mut TextColor), With<AssemblyClearanceText>>,
) {
    if !version.is_changed() && !clearance.is_changed() {
        return;
    }
    let Ok((mut text, mut color)) = texts.single_mut() else {
        return;
    };

    if clearance
        .report
        .as_ref()
        .is_some_and(|report| report.checked_version != version.value)
    {
        **text = "Geometry changed - check clearance again".to_string();
        *color = TextColor(TEXT_WARNING);
        return;
    }

    **text = clearance.message.clone();
    *color = TextColor(match clearance.tone {
        ClearanceTone::Muted => TEXT_MUTED,
        ClearanceTone::Ok => TEXT_OK,
        ClearanceTone::Warning => TEXT_WARNING,
        ClearanceTone::Error => TEXT_ERROR,
    });
}

fn build_boundary_collider(mesh: &FemMesh) -> Result<PartCollider, String> {
    let mut vertex_by_node = BTreeMap::<NodeId, u32>::new();
    let mut vertices = Vec::<Vec3>::new();
    let mut triangles = Vec::<[u32; 3]>::new();

    for face in mesh.cached_boundary_faces() {
        if face.nodes.len() < 3 {
            continue;
        }

        let polygon: Option<Vec<u32>> = face
            .nodes
            .iter()
            .map(|node_id| {
                if let Some(index) = vertex_by_node.get(node_id) {
                    return Some(*index);
                }
                let position = mesh.node_position(*node_id)?;
                if !position.is_finite() {
                    return None;
                }
                let index = u32::try_from(vertices.len()).ok()?;
                vertices.push(position);
                vertex_by_node.insert(*node_id, index);
                Some(index)
            })
            .collect();
        let Some(polygon) = polygon else {
            continue;
        };

        for index in 1..(polygon.len() - 1) {
            let triangle = [polygon[0], polygon[index], polygon[index + 1]];
            let [a, b, c] = triangle.map(|vertex| vertices[vertex as usize]);
            let area_squared = (b - a).cross(c - a).length_squared();
            if area_squared.is_finite() && area_squared > 1.0e-20 {
                triangles.push(triangle);
            }
        }
    }

    if triangles.is_empty() {
        return Err("selected part has no supported boundary faces".to_string());
    }

    let flags = TrimeshFlags::MERGE_DUPLICATE_VERTICES
        | TrimeshFlags::DELETE_DEGENERATE_TRIANGLES
        | TrimeshFlags::DELETE_DUPLICATE_TRIANGLES
        | TrimeshFlags::ORIENTED;
    let shape = Collider::try_trimesh_with_config(vertices, triangles, flags)
        .map_err(|error| format!("could not build boundary collider: {error:?}"))?;
    let interior_sample = mesh
        .elements
        .iter()
        .find(|element| element.element_type.is_solid())
        .and_then(|element| mesh.node_positions(&element.nodes))
        .and_then(|positions| {
            if positions.is_empty() {
                None
            } else {
                Some(
                    positions
                        .iter()
                        .copied()
                        .fold(Vec3::ZERO, |sum, point| sum + point)
                        / positions.len() as f32,
                )
            }
        });

    Ok(PartCollider {
        shape,
        interior_sample,
    })
}

fn evaluate_selected_part(
    model: &FemModel,
    colliders: &[Result<PartCollider, String>],
    selected_part: usize,
    version: u64,
) -> Result<ClearanceReport, String> {
    let selected = colliders
        .get(selected_part)
        .ok_or_else(|| "selected part collider is missing".to_string())?
        .as_ref()
        .map_err(|error| format!("selected part: {error}"))?;

    let mut intersections = Vec::<ClearanceReport>::new();
    let mut touching = Vec::<ClearanceReport>::new();
    let mut nearest: Option<ClearanceReport> = None;
    let mut skipped = 0usize;

    for (other_part, other) in colliders.iter().enumerate() {
        if other_part == selected_part {
            continue;
        }
        let Ok(other) = other.as_ref() else {
            skipped += 1;
            continue;
        };

        let separation = distance(
            &selected.shape,
            Vec3::ZERO,
            Quat::IDENTITY,
            &other.shape,
            Vec3::ZERO,
            Quat::IDENTITY,
        )
        .map_err(|error| format!("Avian distance query failed: {error:?}"))?;
        let tolerance = pair_tolerance(model, selected_part, other_part);

        if separation <= tolerance {
            let intersects = intersection_test(
                &selected.shape,
                Vec3::ZERO,
                Quat::IDENTITY,
                &other.shape,
                Vec3::ZERO,
                Quat::IDENTITY,
            )
            .map_err(|error| format!("Avian intersection query failed: {error:?}"))?;
            let closest_points = contact(
                &selected.shape,
                Vec3::ZERO,
                Quat::IDENTITY,
                &other.shape,
                Vec3::ZERO,
                Quat::IDENTITY,
                tolerance,
            )
            .ok()
            .flatten()
            .map(|contact| (contact.local_point1, contact.local_point2));
            let report = ClearanceReport {
                checked_version: version,
                selected_part,
                other_part,
                kind: if intersects {
                    ClearanceKind::Intersecting
                } else {
                    ClearanceKind::Touching
                },
                distance: separation,
                affected_parts: 1,
                closest_points,
            };
            if intersects {
                intersections.push(report);
            } else {
                touching.push(report);
            }
            continue;
        }

        if parts_contain_each_other(selected, other) {
            intersections.push(ClearanceReport {
                checked_version: version,
                selected_part,
                other_part,
                kind: ClearanceKind::Intersecting,
                distance: 0.0,
                affected_parts: 1,
                closest_points: None,
            });
            continue;
        }

        let closest_points = closest_points(
            &selected.shape,
            Vec3::ZERO,
            Quat::IDENTITY,
            &other.shape,
            Vec3::ZERO,
            Quat::IDENTITY,
            separation + tolerance,
        )
        .ok()
        .and_then(|points| match points {
            ClosestPoints::WithinMargin(a, b) => Some((a, b)),
            ClosestPoints::Intersecting | ClosestPoints::OutsideMargin => None,
        });
        let report = ClearanceReport {
            checked_version: version,
            selected_part,
            other_part,
            kind: ClearanceKind::Separated,
            distance: separation,
            affected_parts: 1,
            closest_points,
        };
        if nearest
            .as_ref()
            .is_none_or(|current| report.distance < current.distance)
        {
            nearest = Some(report);
        }
    }

    let intersection_count = intersections.len();
    if let Some(mut report) = intersections.into_iter().next() {
        report.affected_parts = intersection_count;
        return Ok(report);
    }
    let touching_count = touching.len();
    if let Some(mut report) = touching.into_iter().next() {
        report.affected_parts = touching_count;
        return Ok(report);
    }
    if let Some(report) = nearest {
        return Ok(report);
    }

    if skipped > 0 {
        Err(format!(
            "No comparable boundary surface; {skipped} part(s) were skipped"
        ))
    } else {
        Err("No other part is available for comparison".to_string())
    }
}

fn parts_contain_each_other(a: &PartCollider, b: &PartCollider) -> bool {
    a.interior_sample
        .is_some_and(|point| b.shape.contains_point(Vec3::ZERO, Quat::IDENTITY, point))
        || b.interior_sample
            .is_some_and(|point| a.shape.contains_point(Vec3::ZERO, Quat::IDENTITY, point))
}

fn pair_tolerance(model: &FemModel, part_a: usize, part_b: usize) -> f32 {
    let diagonal = [part_a, part_b]
        .into_iter()
        .filter_map(|part| model.part_bounds(part))
        .map(|(min, max)| min.distance(max))
        .fold(0.0_f32, f32::max);
    (diagonal * 1.0e-6).max(1.0e-7)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_demo_parts(offset: Vec3) -> FemModel {
        let mut model = FemModel::single_mesh("A", FemMesh::demo_hex8());
        model.add_mesh("B", FemMesh::demo_hex8());
        assert!(model.translate_part(1, offset));
        model
    }

    #[test]
    fn boundary_collider_reports_cube_clearance() {
        let model = two_demo_parts(Vec3::X * 3.0);
        let colliders: Vec<_> = model
            .parts
            .iter()
            .map(|part| build_boundary_collider(&model.meshes[part.mesh_index]))
            .collect();

        let report = evaluate_selected_part(&model, &colliders, 0, 7).unwrap();

        assert_eq!(report.kind, ClearanceKind::Separated);
        assert!((report.distance - 1.0).abs() < 1.0e-5);
        assert_eq!(report.other_part, 1);
        assert_eq!(report.checked_version, 7);
    }

    #[test]
    fn boundary_collider_detects_crossing_surfaces() {
        let model = two_demo_parts(Vec3::new(1.2, 0.1, 0.1));
        let colliders: Vec<_> = model
            .parts
            .iter()
            .map(|part| build_boundary_collider(&model.meshes[part.mesh_index]))
            .collect();

        let report = evaluate_selected_part(&model, &colliders, 0, 1).unwrap();

        assert_eq!(report.kind, ClearanceKind::Intersecting);
        assert_eq!(report.distance, 0.0);
    }

    #[test]
    fn boundary_collider_detects_full_containment() {
        let mut inner = FemMesh::demo_hex8();
        for node in &mut inner.nodes {
            node.position *= 0.4;
        }
        inner.rebuild_topology_cache();
        let mut model = FemModel::single_mesh("Outer", FemMesh::demo_hex8());
        model.add_mesh("Inner", inner);
        let colliders: Vec<_> = model
            .parts
            .iter()
            .map(|part| build_boundary_collider(&model.meshes[part.mesh_index]))
            .collect();

        let report = evaluate_selected_part(&model, &colliders, 0, 3).unwrap();

        assert_eq!(report.kind, ClearanceKind::Intersecting);
        assert_eq!(report.distance, 0.0);
    }
}
