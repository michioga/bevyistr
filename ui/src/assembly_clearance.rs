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

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AssemblyClearanceReviewButton {
    action: ClearanceReviewAction,
}

#[derive(Debug, Clone, Copy)]
enum ClearanceReviewAction {
    Previous,
    Next,
}

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
    closest_points: Option<(Vec3, Vec3)>,
}

struct ClearanceEvaluation {
    reports: Vec<ClearanceReport>,
    skipped_parts: usize,
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
    reports: Vec<ClearanceReport>,
    selected_report: Option<usize>,
    skipped_parts: usize,
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
            reports: Vec::new(),
            selected_report: None,
            skipped_parts: 0,
            message: "Select a part, then check clearance".to_string(),
            tone: ClearanceTone::Muted,
        }
    }
}

impl AssemblyClearanceState {
    fn active_report(&self) -> Option<&ClearanceReport> {
        self.selected_report
            .and_then(|index| self.reports.get(index))
    }

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
        self.reports.clear();
        self.selected_report = None;
        self.skipped_parts = 0;

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
        let evaluation =
            match evaluate_selected_part(model, &self.colliders, selected_part, version) {
                Ok(evaluation) => evaluation,
                Err(error) => {
                    self.set_message(error, ClearanceTone::Error);
                    return;
                }
            };

        self.reports = evaluation.reports;
        self.selected_report = Some(0);
        self.skipped_parts = evaluation.skipped_parts;
        self.refresh_message(model);
    }

    fn navigate(&mut self, action: ClearanceReviewAction, model: &FemModel) {
        if self.reports.is_empty() {
            return;
        }
        let current = self
            .selected_report
            .unwrap_or(0)
            .min(self.reports.len() - 1);
        self.selected_report = Some(match action {
            ClearanceReviewAction::Previous => {
                current.checked_sub(1).unwrap_or(self.reports.len() - 1)
            }
            ClearanceReviewAction::Next => (current + 1) % self.reports.len(),
        });
        self.refresh_message(model);
    }

    fn refresh_message(&mut self, model: &FemModel) {
        let Some(report) = self.active_report().cloned() else {
            return;
        };

        let selected_name = &model.parts[report.selected_part].name;
        let other_name = &model.parts[report.other_part].name;
        let position = self.selected_report.unwrap_or(0) + 1;
        let total = self.reports.len();
        let skipped = if self.skipped_parts > 0 {
            format!("\n{} unsupported part(s) skipped", self.skipped_parts)
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
                        "Clearance: {:.6} model units  [{position}/{total}]\n{} -> [{}] {}\nGap vector: ({:.6}, {:.6}, {:.6}){skipped}",
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
                        "Clearance: {:.6} model units  [{position}/{total}]\n{} -> [{}] {}{skipped}",
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
                    "TOUCHING (within tolerance)  [{position}/{total}]\n{} -> [{}] {}{skipped}",
                    selected_name,
                    report.other_part + 1,
                    other_name
                ),
                ClearanceTone::Warning,
            ),
            ClearanceKind::Intersecting => self.set_message(
                format!(
                    "INTERFERENCE DETECTED  [{position}/{total}]\n{} -> [{}] {}{skipped}",
                    selected_name,
                    report.other_part + 1,
                    other_name
                ),
                ClearanceTone::Error,
            ),
        }
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

    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(6.0),
            ..default()
        },))
        .with_children(|row| {
            for (label, action) in [
                ("Previous pair", ClearanceReviewAction::Previous),
                ("Next pair", ClearanceReviewAction::Next),
            ] {
                row.spawn((
                    Button,
                    Node {
                        flex_grow: 1.0,
                        height: px(27.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(5.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(PANEL_BORDER),
                    AssemblyClearanceReviewButton { action },
                    Name::new(format!("AssemblyClearance{label}")),
                ))
                .with_child((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(TEXT_MAIN),
                ));
            }
        });

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
        .active_report()
        .filter(|report| report.checked_version == version.value)
    else {
        return;
    };
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

    draw_part_bounds(
        &mut gizmos,
        &model,
        report.selected_part,
        Color::srgb(0.20, 0.72, 1.0),
    );
    draw_part_bounds(&mut gizmos, &model, report.other_part, color);

    let Some((selected_point, other_point)) = report.closest_points else {
        return;
    };
    if !selected_point.is_finite() || !other_point.is_finite() {
        return;
    }

    gizmos.sphere(selected_point, marker_radius, color);
    if selected_point.distance_squared(other_point) > marker_radius * marker_radius * 0.01 {
        gizmos.line(selected_point, other_point, color);
        gizmos.sphere(other_point, marker_radius, color);
    }
}

fn draw_part_bounds(
    gizmos: &mut Gizmos<AssemblyClearanceGizmos>,
    model: &FemModel,
    part_index: usize,
    color: Color,
) {
    let Some((min, max)) = model.part_bounds(part_index) else {
        return;
    };
    let size = max - min;
    if !size.is_finite() || size.max_element() <= 1.0e-9 {
        return;
    }
    gizmos.cube(
        Transform::from_translation((min + max) * 0.5).with_scale(size * 1.002),
        color,
    );
}

pub(crate) fn assembly_clearance_review_button_system(
    model: Res<FemModel>,
    mut clearance: ResMut<AssemblyClearanceState>,
    mut buttons: Query<(
        Ref<Interaction>,
        &AssemblyClearanceReviewButton,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (interaction, button, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            clearance.navigate(button.action, &model);
        }

        *background = BackgroundColor(match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        });
        *border = BorderColor::all(PANEL_BORDER);
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
        .active_report()
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
) -> Result<ClearanceEvaluation, String> {
    let selected = colliders
        .get(selected_part)
        .ok_or_else(|| "selected part collider is missing".to_string())?
        .as_ref()
        .map_err(|error| format!("selected part: {error}"))?;

    let mut reports = Vec::<ClearanceReport>::new();
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
                closest_points,
            };
            reports.push(report);
            continue;
        }

        if parts_contain_each_other(selected, other) {
            reports.push(ClearanceReport {
                checked_version: version,
                selected_part,
                other_part,
                kind: ClearanceKind::Intersecting,
                distance: 0.0,
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
            closest_points,
        };
        reports.push(report);
    }

    if reports.is_empty() {
        return if skipped > 0 {
            Err(format!(
                "No comparable boundary surface; {skipped} part(s) were skipped"
            ))
        } else {
            Err("No other part is available for comparison".to_string())
        };
    }

    reports.sort_by(|a, b| {
        clearance_priority(a.kind)
            .cmp(&clearance_priority(b.kind))
            .then_with(|| a.distance.total_cmp(&b.distance))
            .then_with(|| a.other_part.cmp(&b.other_part))
    });
    Ok(ClearanceEvaluation {
        reports,
        skipped_parts: skipped,
    })
}

fn clearance_priority(kind: ClearanceKind) -> u8 {
    match kind {
        ClearanceKind::Intersecting => 0,
        ClearanceKind::Touching => 1,
        ClearanceKind::Separated => 2,
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

        let evaluation = evaluate_selected_part(&model, &colliders, 0, 7).unwrap();
        let report = &evaluation.reports[0];

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

        let evaluation = evaluate_selected_part(&model, &colliders, 0, 1).unwrap();
        let report = &evaluation.reports[0];

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

        let evaluation = evaluate_selected_part(&model, &colliders, 0, 3).unwrap();
        let report = &evaluation.reports[0];

        assert_eq!(report.kind, ClearanceKind::Intersecting);
        assert_eq!(report.distance, 0.0);
    }

    #[test]
    fn clearance_reports_are_reviewed_by_risk_then_distance() {
        let mut model = FemModel::single_mesh("Selected", FemMesh::demo_hex8());
        model.add_mesh("Far", FemMesh::demo_hex8());
        model.add_mesh("Intersecting", FemMesh::demo_hex8());
        model.add_mesh("Near", FemMesh::demo_hex8());
        assert!(model.translate_part(1, Vec3::X * 6.0));
        assert!(model.translate_part(2, Vec3::X * 1.2));
        assert!(model.translate_part(3, Vec3::X * 3.0));
        let colliders: Vec<_> = model
            .parts
            .iter()
            .map(|part| build_boundary_collider(&model.meshes[part.mesh_index]))
            .collect();

        let evaluation = evaluate_selected_part(&model, &colliders, 0, 4).unwrap();

        assert_eq!(evaluation.reports.len(), 3);
        assert_eq!(evaluation.reports[0].kind, ClearanceKind::Intersecting);
        assert_eq!(evaluation.reports[0].other_part, 2);
        assert_eq!(evaluation.reports[1].kind, ClearanceKind::Separated);
        assert_eq!(evaluation.reports[1].other_part, 3);
        assert_eq!(evaluation.reports[2].other_part, 1);
    }

    #[test]
    fn clearance_review_wraps_in_both_directions() {
        let mut model = FemModel::single_mesh("Selected", FemMesh::demo_hex8());
        model.add_mesh("Near", FemMesh::demo_hex8());
        model.add_mesh("Far", FemMesh::demo_hex8());
        assert!(model.translate_part(1, Vec3::X * 3.0));
        assert!(model.translate_part(2, Vec3::X * 5.0));
        let mut state = AssemblyClearanceState::default();

        state.check(&model, 9, Some(0));
        assert_eq!(state.active_report().unwrap().other_part, 1);
        state.navigate(ClearanceReviewAction::Next, &model);
        assert_eq!(state.active_report().unwrap().other_part, 2);
        state.navigate(ClearanceReviewAction::Next, &model);
        assert_eq!(state.active_report().unwrap().other_part, 1);
        state.navigate(ClearanceReviewAction::Previous, &model);
        assert_eq!(state.active_report().unwrap().other_part, 2);
    }
}
