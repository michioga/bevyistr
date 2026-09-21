//! A read-only result identity, scoped to one loaded model and one local piece.
//! No geometry queries are needed to follow a pinned sample through playback.
use crate::{layout::SidebarPage, results_ui::PlaybackState};
use bevy::{
    prelude::*,
    ui_widgets::{Activate, Button as WidgetButton},
    window::PrimaryWindow,
};
use fem_core::{FemMesh, FemModel, FemResultSet, ResultField, ResultGeometry, UiPointerState};
use visualization::{VisualizationSettings, result_probe::ProbeHit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Node {
        index: usize,
        id: fem_core::NodeId,
    },
    Element {
        index: usize,
        id: fem_core::ElementId,
    },
}

impl Target {
    fn from_hit(mesh: &FemMesh, hit: &ProbeHit) -> Option<Self> {
        if let Some(id) = hit.node {
            Some(Self::Node {
                index: mesh.nodes.iter().position(|n| n.id == id)?,
                id,
            })
        } else {
            let id = hit.element?;
            Some(Self::Element {
                index: mesh.elements.iter().position(|e| e.id == id)?,
                id,
            })
        }
    }

    pub(crate) fn value(self, field: Option<&ResultField>) -> Option<f32> {
        let value = match (self, field?) {
            (Self::Node { index, .. }, ResultField::NodeScalar { values, .. }) => {
                *values.get(index)?
            }
            (Self::Node { index, .. }, ResultField::NodeVector { values, .. }) => {
                values.get(index)?.length()
            }
            (Self::Element { index, .. }, ResultField::ElementScalar { values, .. }) => {
                *values.get(index)?
            }
            _ => return None, // Never convert nodal values to element values or vice versa.
        };
        value.is_finite().then_some(value)
    }

    pub(crate) fn label(self) -> String {
        match self {
            Self::Node { id, .. } => format!("Node {}", id.0),
            Self::Element { id, .. } => format!("Element {}", id.0),
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct ProbePin {
    pub hovered: Option<(usize, ProbeHit)>,
    pinned: Option<(usize, Target)>,
    press: Option<Vec2>,
    generation: u64,
}

impl ProbePin {
    #[cfg(test)]
    pub(crate) fn for_test(part: usize, target: Target) -> Self {
        Self {
            pinned: Some((part, target)),
            ..default()
        }
    }

    pub fn clear(&mut self) {
        if self.pinned.is_some() {
            self.generation = self.generation.wrapping_add(1);
        }
        self.pinned = None;
        self.press = None;
    }

    pub(crate) fn selection(&self) -> Option<(u64, usize, Target)> {
        self.pinned
            .map(|(part, target)| (self.generation, part, target))
    }

    fn click(
        &mut self,
        position: Option<Vec2>,
        pressed: bool,
        released: bool,
        allowed: bool,
    ) -> bool {
        let Some(position) = position.filter(|_| allowed) else {
            self.press = None;
            return false;
        };
        if pressed {
            self.press = Some(position);
        }
        // Cancel the entire gesture once it becomes a drag, even if it returns.
        if self
            .press
            .is_some_and(|p| p.distance_squared(position) > 16.0)
        {
            self.press = None;
        }
        if released {
            self.press.take().is_some()
        } else {
            false
        }
    }
}

#[derive(Component)]
pub(crate) struct PinText;
#[derive(Component)]
pub(crate) struct ClearPin;

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        PinText,
        Pickable::IGNORE,
        Text::new("PROBE | Pause, then click the result to pin a value."),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(Color::WHITE),
    ));
    parent
        .spawn((
            Button,
            WidgetButton,
            ClearPin,
            bevy::input_focus::tab_navigation::TabIndex(0),
            Node {
                width: percent(100),
                min_height: px(26),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.14, 0.17)),
            BorderColor::all(Color::srgb(0.35, 0.55, 0.62)),
        ))
        .observe(|_: On<Activate>, mut pin: ResMut<ProbePin>| pin.clear())
        .with_child((
            Text::new("Clear pinned probe"),
            Pickable::IGNORE,
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ));
    crate::probe_comparison::spawn(parent);
    crate::probe_history::spawn(parent);
    crate::probe_history_csv::spawn(parent);
}

fn describe(
    part: usize,
    target: Target,
    results: &FemResultSet,
    contour: &visualization::ContourSettings,
) -> String {
    let step = results
        .by_mesh
        .get(part)
        .and_then(|s| s.get(contour.step_index));
    let field = step.and_then(|s| s.field_by_name(&contour.field_name));
    let value = target
        .value(field)
        .map_or_else(|| "unavailable".into(), |v| format!("{v:.6e}"));
    let time = step.map_or_else(
        || "Frame unavailable".into(),
        |s| format!("Step {} | Time {:.6e}", s.step, s.time),
    );
    let quantity = if matches!(field, Some(ResultField::NodeVector { .. })) {
        "Magnitude"
    } else {
        "Value"
    };
    format!(
        "PINNED | Part {} | {}\n{}\n{quantity}: {value}\n{time}\nResult/model units | click another point to replace",
        part + 1,
        target.label(),
        contour.field_name
    )
}

pub(crate) fn update(
    page: Res<SidebarPage>,
    playback: Res<PlaybackState>,
    pointer: Res<UiPointerState>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    model: Option<Res<FemModel>>,
    geometry: Res<ResultGeometry>,
    results: Res<FemResultSet>,
    settings: Res<VisualizationSettings>,
    mut pin: ResMut<ProbePin>,
    mut text: Query<&mut Text, With<PinText>>,
    mut buttons: Query<&mut Node, With<ClearPin>>,
) {
    let allowed = *page == SidebarPage::Results
        && geometry.visible
        && !playback.playing
        && !pointer.over_ui
        && !mouse.pressed(MouseButton::Middle)
        && !mouse.pressed(MouseButton::Right);
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());
    if pin.click(
        cursor,
        mouse.just_pressed(MouseButton::Left),
        mouse.just_released(MouseButton::Left),
        allowed,
    ) {
        if let Some((part, hit)) = &pin.hovered {
            if let Some(mesh) = geometry
                .model
                .as_ref()
                .or(model.as_deref())
                .and_then(|m| m.meshes.get(*part))
            {
                pin.pinned = Target::from_hit(mesh, hit).map(|target| (*part, target));
            }
        }
    }
    if !results.has_results() {
        pin.clear();
    }
    let label = match (pin.pinned, &settings.contour) {
        (Some((part, target)), Some(contour)) => describe(part, target, &results, contour),
        _ => "PROBE | Pause, then click the result to pin a value.".into(),
    };
    for mut text in &mut text {
        text.set_if_neq(Text::new(label.clone()));
    }
    for mut node in &mut buttons {
        let display = if pin.pinned.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clearing_and_repinning_the_same_id_invalidates_history_identity() {
        let target = Target::Node {
            index: 0,
            id: fem_core::NodeId(1),
        };
        let mut pin = ProbePin::for_test(0, target);
        let first = pin.selection();
        pin.clear();
        pin.pinned = Some((0, target));
        assert_ne!(first, pin.selection());
    }

    #[test]
    fn clear_button_uses_normal_widget_activation() {
        let mut app = App::new();
        app.add_plugins(bevy::ui_widgets::ButtonPlugin)
            .init_resource::<ProbePin>()
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(spawn);
            });
        app.update();
        app.world_mut().resource_mut::<ProbePin>().pinned = Some((
            1,
            Target::Element {
                index: 0,
                id: fem_core::ElementId(1),
            },
        ));
        let button = app
            .world_mut()
            .query_filtered::<Entity, With<ClearPin>>()
            .single(app.world())
            .unwrap();
        crate::widget_test_input::click(app.world_mut(), button);
        assert!(app.world().resource::<ProbePin>().pinned.is_none());
    }
    #[test]
    fn click_requires_viewport_press_and_release_without_dragging() {
        let mut pin = ProbePin::default();
        assert!(!pin.click(Some(Vec2::ZERO), false, true, true));
        assert!(!pin.click(Some(Vec2::ZERO), true, false, true));
        assert!(pin.click(Some(Vec2::ONE), false, true, true));
        pin.click(Some(Vec2::ZERO), true, false, true);
        pin.click(Some(Vec2::X * 8.0), false, false, true);
        assert!(!pin.click(Some(Vec2::ZERO), false, true, true));
        pin.click(Some(Vec2::ZERO), true, false, true);
        pin.click(Some(Vec2::ZERO), false, false, false);
        assert!(!pin.click(Some(Vec2::ZERO), false, true, true));
    }

    #[test]
    fn target_preserves_association_and_reports_missing_not_zero() {
        let target = Target::Node {
            index: 1,
            id: fem_core::NodeId(9),
        };
        let scalar = ResultField::NodeScalar {
            name: "P".into(),
            values: vec![10., 20.],
            min: 10.,
            max: 20.,
        };
        assert_eq!(target.value(Some(&scalar)), Some(20.));
        assert_eq!(target.value(None), None);
        let element = ResultField::ElementScalar {
            name: "P".into(),
            values: vec![10., 20.],
            min: 10.,
            max: 20.,
        };
        assert_eq!(target.value(Some(&element)), None);
        let vector = ResultField::NodeVector {
            name: "U".into(),
            values: vec![Vec3::ZERO, Vec3::new(3., 4., 0.)],
            min_mag: 0.,
            max_mag: 5.,
        };
        assert_eq!(target.value(Some(&vector)), Some(5.));
        let bad = ResultField::NodeScalar {
            name: "P".into(),
            values: vec![1., f32::NAN],
            min: 1.,
            max: 1.,
        };
        assert_eq!(target.value(Some(&bad)), None);
    }

    #[test]
    fn same_local_id_in_other_piece_is_never_sampled_and_frames_follow() {
        let scalar = |value| fem_core::StepResult {
            step: value as u32,
            time: value,
            fields: vec![ResultField::ElementScalar {
                name: "E".into(),
                values: vec![value],
                min: value,
                max: value,
            }],
        };
        let results = FemResultSet {
            by_mesh: vec![vec![scalar(99.)], vec![scalar(2.), scalar(3.)]],
            ..default()
        };
        let target = Target::Element {
            index: 0,
            id: fem_core::ElementId(1),
        };
        let mut contour = visualization::ContourSettings {
            mesh_index: 0,
            step_index: 0,
            field_name: "E".into(),
            show_deformation: true,
            displacement_field: "U".into(),
            deformation_scale: 20.,
        };
        assert!(describe(1, target, &results, &contour).contains("Value: 2.000000e0"));
        contour.step_index = 1;
        assert!(describe(1, target, &results, &contour).contains("Value: 3.000000e0"));
        contour.step_index = 2;
        assert!(describe(1, target, &results, &contour).contains("Frame unavailable"));
    }
}
