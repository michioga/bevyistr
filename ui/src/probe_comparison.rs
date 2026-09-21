//! Explicit, bounded comparison of part-local identities. Never modifies the pin.
use crate::{
    layout::SidebarPage,
    result_probe_pin::{ProbePin, Target},
};
use bevy::{
    prelude::*,
    ui_widgets::{Activate, Button as WidgetButton},
};
use fem_core::{FemModel, FemModelVersion, FemResultSet, ResultField, ResultGeometry, StepResult};
use visualization::VisualizationSettings;

pub(crate) const COLORS: [[u8; 4]; 4] = [
    [65, 200, 235, 255],
    [255, 160, 90, 255],
    [205, 145, 245, 255],
    [140, 220, 125, 255],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub slot: usize,
    pub part: usize,
    pub target: Target,
}

#[derive(Resource, Default)]
pub(crate) struct Comparison {
    pub entries: Vec<Entry>,
    pub revision: u64,
    status: String,
    field: String,
}

// Compare recorded step/time metadata, not merely frame counts or local IDs.
fn aligned(a: &[StepResult], b: &[StepResult]) -> bool {
    !a.is_empty()
        && a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.step == b.step && (a.time == b.time || (a.time.is_nan() && b.time.is_nan()))
        })
}

fn quantity(steps: &[StepResult], target: Target, field: &str) -> Option<u8> {
    let mut kind = None;
    for step in steps {
        let Some(f) = step.field_by_name(field) else {
            continue;
        };
        let current = match (target, f) {
            (Target::Node { .. }, ResultField::NodeScalar { .. }) => 0,
            (Target::Node { .. }, ResultField::NodeVector { .. }) => 1,
            (Target::Element { .. }, ResultField::ElementScalar { .. }) => 2,
            _ => return None,
        };
        if kind.is_some_and(|k| k != current) {
            return None;
        }
        kind = Some(current);
    }
    kind
}

impl Comparison {
    fn check(
        &self,
        part: usize,
        target: Target,
        results: &FemResultSet,
        field: &str,
    ) -> Result<(), &'static str> {
        let steps = results.by_mesh.get(part).ok_or("Result part unavailable")?;
        let kind = quantity(steps, target, field).ok_or("Select a target matching this field")?;
        if let Some(first) = self.entries.first() {
            let reference = results
                .by_mesh
                .get(first.part)
                .ok_or("Result part unavailable")?;
            if !aligned(reference, steps) {
                return Err("Cannot compare different Step / Time sequences");
            }
            if quantity(reference, first.target, field) != Some(kind) {
                return Err("Cannot mix node / element or scalar / magnitude values");
            }
        }
        Ok(())
    }

    fn add(
        &mut self,
        pin: &ProbePin,
        results: &FemResultSet,
        field: &str,
    ) -> Result<(), &'static str> {
        let (_, part, target) = pin
            .selection()
            .ok_or("Click a result point to pin it first")?;
        if self
            .entries
            .iter()
            .any(|e| e.part == part && e.target == target)
        {
            return Err("Already in comparison");
        }
        if self.entries.len() == COLORS.len() {
            return Err("Maximum 4 targets; remove one to add another");
        }
        self.check(part, target, results, field)?;
        let slot = (0..COLORS.len())
            .find(|s| self.entries.iter().all(|e| e.slot != *s))
            .unwrap();
        self.entries.push(Entry { slot, part, target });
        self.revision = self.revision.wrapping_add(1);
        self.field = field.into();
        self.status.clear();
        Ok(())
    }

    fn remove(&mut self, slot: usize) {
        self.entries.retain(|e| e.slot != slot);
        self.revision = self.revision.wrapping_add(1);
        self.status.clear();
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.revision = self.revision.wrapping_add(1);
        self.status.clear();
    }
}

#[derive(Component)]
pub(crate) enum Action {
    Add,
    Remove(usize),
    Clear,
}
#[derive(Component)]
pub(crate) struct Row(usize);
#[derive(Component)]
pub(crate) struct RowText(usize);
#[derive(Component)]
pub(crate) struct Status;

fn text(value: &str) -> impl Bundle {
    (
        Text::new(value),
        Pickable::IGNORE,
        TextFont {
            font_size: FontSize::Px(10.),
            ..default()
        },
        TextColor(Color::WHITE),
    )
}

fn button(parent: &mut ChildSpawnerCommands, action: Action, title: &str) {
    parent
        .spawn((
            Button,
            WidgetButton,
            action,
            bevy::input_focus::tab_navigation::TabIndex(0),
            Node {
                min_height: px(26),
                padding: UiRect::all(px(4)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.14, 0.17)),
            BorderColor::all(Color::srgb(0.35, 0.55, 0.62)),
        ))
        .observe(activate)
        .with_child(text(title));
}

fn activate(
    event: On<Activate>,
    actions: Query<&Action>,
    page: Res<SidebarPage>,
    pin: Res<ProbePin>,
    results: Res<FemResultSet>,
    settings: Res<VisualizationSettings>,
    mut comparison: ResMut<Comparison>,
) {
    if *page != SidebarPage::Results {
        return;
    }
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    match action {
        Action::Add => {
            let Some(contour) = &settings.contour else {
                return;
            };
            if let Err(reason) = comparison.add(&pin, &results, &contour.field_name) {
                comparison.status = reason.into();
            }
        }
        Action::Remove(slot) => comparison.remove(*slot),
        Action::Clear => comparison.clear(),
    }
}

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    button(parent, Action::Add, "Add pinned probe to comparison");
    parent.spawn((Status, text("")));
    for slot in 0..COLORS.len() {
        parent
            .spawn((
                Row(slot),
                Node {
                    display: Display::None,
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(4),
                    ..default()
                },
            ))
            .with_children(|row| {
                let [r, g, b, _] = COLORS[slot];
                row.spawn((
                    Node {
                        width: px(8),
                        height: px(8),
                        flex_shrink: 0.,
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(r, g, b)),
                    Pickable::IGNORE,
                ));
                row.spawn((
                    RowText(slot),
                    text(""),
                    Node {
                        flex_grow: 1.,
                        flex_basis: px(0),
                        ..default()
                    },
                ));
                button(row, Action::Remove(slot), "Remove");
            });
    }
    button(parent, Action::Clear, "Clear comparison");
}

pub(crate) fn update(
    mut commands: Commands,
    page: Res<SidebarPage>,
    pin: Res<ProbePin>,
    results: Res<FemResultSet>,
    settings: Res<VisualizationSettings>,
    geometry: Res<ResultGeometry>,
    version: Res<FemModelVersion>,
    model: Option<Res<FemModel>>,
    mut comparison: ResMut<Comparison>,
    mut rows: Query<(&Row, &mut Node), Without<Action>>,
    mut buttons: Query<
        (
            Entity,
            &Action,
            &mut Node,
            &Interaction,
            &mut BackgroundColor,
            Option<&bevy::ui::InteractionDisabled>,
        ),
        Without<Row>,
    >,
    mut labels: Query<(&RowText, &mut Text), Without<Status>>,
    mut statuses: Query<&mut Text, With<Status>>,
) {
    if !comparison.entries.is_empty()
        && (*page != SidebarPage::Results
            || !results.has_results()
            || geometry.is_changed()
            || version.is_changed()
            || model.as_ref().is_some_and(|m| m.is_changed()))
    {
        comparison.clear();
    }
    let contour = settings.contour.as_ref();
    if let Some(c) = contour {
        if comparison.field != c.field_name {
            let invalid = comparison.entries.iter().find_map(|e| {
                comparison
                    .check(e.part, e.target, &results, &c.field_name)
                    .err()
            });
            if let Some(reason) = invalid {
                comparison.clear();
                comparison.status = format!("Comparison cleared: {reason}");
            }
            comparison.field = c.field_name.clone();
        }
    }
    for (row, mut node) in &mut rows {
        let visible = comparison.entries.iter().any(|e| e.slot == row.0);
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (entity, action, mut node, interaction, mut bg, inactive) in &mut buttons {
        let disabled = match action {
            Action::Add => {
                pin.selection().is_none() || contour.is_none() || *page != SidebarPage::Results
            }
            _ => false,
        };
        if disabled && inactive.is_none() {
            commands
                .entity(entity)
                .insert(bevy::ui::InteractionDisabled);
        } else if !disabled && inactive.is_some() {
            commands
                .entity(entity)
                .remove::<bevy::ui::InteractionDisabled>();
        }
        if matches!(action, Action::Clear) {
            node.display = if comparison.entries.is_empty() {
                Display::None
            } else {
                Display::Flex
            };
        }
        bg.set_if_neq(BackgroundColor(if disabled {
            Color::srgb(0.07, 0.09, 0.10)
        } else if *interaction != Interaction::None {
            Color::srgb(0.18, 0.32, 0.39)
        } else {
            Color::srgb(0.10, 0.14, 0.17)
        }));
    }
    for (label, mut label_text) in &mut labels {
        if let Some(e) = comparison.entries.iter().find(|e| e.slot == label.0) {
            let value = contour
                .and_then(|c| {
                    results
                        .by_mesh
                        .get(e.part)?
                        .get(c.step_index)?
                        .field_by_name(&c.field_name)
                })
                .and_then(|f| e.target.value(Some(f)))
                .map_or_else(|| "unavailable".into(), |v| format!("{v:.6e}"));
            label_text.set_if_neq(Text::new(format!(
                "{} | Part {} | {}\n{value}",
                e.slot + 1,
                e.part + 1,
                e.target.label()
            )));
        }
    }
    let message = if !comparison.status.is_empty() {
        comparison.status.clone()
    } else if comparison.entries.is_empty() {
        "Click to pin; Add keeps a target for comparison (max 4).".into()
    } else {
        format!(
            "COMPARISON | {} | {} targets\nShared axes | result/model units. Click replaces PINNED only. CSV exports PINNED only.",
            comparison.field,
            comparison.entries.len()
        )
    };
    for mut status in &mut statuses {
        status.set_if_neq(Text::new(message.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fem_core::NodeId;

    fn target(index: usize) -> Target {
        Target::Node {
            index,
            id: NodeId(index as u32 + 1),
        }
    }
    fn data() -> FemResultSet {
        let steps = [0., 0.25, 1.]
            .into_iter()
            .enumerate()
            .map(|(i, time)| StepResult {
                step: i as u32,
                time,
                fields: vec![ResultField::NodeScalar {
                    name: "P".into(),
                    values: vec![time; 5],
                    min: time,
                    max: time,
                }],
            })
            .collect::<Vec<_>>();
        FemResultSet {
            by_mesh: vec![steps.clone(), steps],
            ..default()
        }
    }
    fn settings() -> VisualizationSettings {
        VisualizationSettings {
            contour: Some(visualization::ContourSettings {
                field_name: "P".into(),
                mesh_index: 0,
                step_index: 0,
                show_deformation: false,
                displacement_field: "U".into(),
                deformation_scale: 1.,
            }),
            ..default()
        }
    }

    #[test]
    fn local_identity_dedup_capacity_and_stable_colors() {
        let results = data();
        let mut c = Comparison::default();
        let pin = ProbePin::for_test(0, target(0));
        c.add(&pin, &results, "P").unwrap();
        assert_eq!(c.add(&pin, &results, "P"), Err("Already in comparison"));
        c.add(&ProbePin::for_test(1, target(0)), &results, "P")
            .unwrap();
        for i in 1..3 {
            c.add(&ProbePin::for_test(0, target(i)), &results, "P")
                .unwrap();
        }
        assert!(
            c.add(&ProbePin::for_test(0, target(3)), &results, "P")
                .is_err()
        );
        let original = c.entries.clone();
        c.remove(1);
        assert_eq!(c.entries[1..], original[2..]);
        c.add(&ProbePin::for_test(0, target(3)), &results, "P")
            .unwrap();
        assert_eq!(c.entries.last().unwrap().slot, 1);
        assert_eq!(pin.selection().unwrap().2, target(0));
    }

    #[test]
    fn refuses_unaligned_series_and_incompatible_quantities_without_mutation() {
        let mut results = data();
        let mut c = Comparison::default();
        c.add(&ProbePin::for_test(0, target(0)), &results, "P")
            .unwrap();
        let before = c.entries.clone();
        results.by_mesh[1][1].time = 0.5;
        assert!(
            c.add(&ProbePin::for_test(1, target(0)), &results, "P")
                .is_err()
        );
        results.by_mesh[1][1].time = 0.25;
        results.by_mesh[1][1].step = 9;
        assert!(
            c.add(&ProbePin::for_test(1, target(0)), &results, "P")
                .is_err()
        );
        results.by_mesh[1][1].step = 1;
        results.by_mesh[1][1].fields = vec![ResultField::NodeVector {
            name: "P".into(),
            values: vec![Vec3::ONE; 5],
            min_mag: 1.,
            max_mag: 1.,
        }];
        assert!(
            c.add(&ProbePin::for_test(1, target(0)), &results, "P")
                .is_err()
        );
        assert!(
            c.add(
                &ProbePin::for_test(
                    0,
                    Target::Element {
                        index: 0,
                        id: fem_core::ElementId(1)
                    }
                ),
                &results,
                "P"
            )
            .is_err()
        );
        assert!(c.add(&ProbePin::default(), &results, "P").is_err());
        assert_eq!(c.entries, before);
    }

    #[test]
    fn missing_values_and_repeated_times_remain_comparable_without_filling() {
        let mut results = data();
        for steps in &mut results.by_mesh {
            for step in steps {
                step.time = 0.;
            }
        }
        results.by_mesh[1][1].fields.clear();
        if let ResultField::NodeScalar { values, .. } = &mut results.by_mesh[1][2].fields[0] {
            values[0] = f32::NAN;
        }
        let mut c = Comparison::default();
        c.add(&ProbePin::for_test(0, target(0)), &results, "P")
            .unwrap();
        c.add(&ProbePin::for_test(1, target(0)), &results, "P")
            .unwrap();
        let samples = crate::probe_history_data::samples(&results.by_mesh[1], target(0), "P");
        assert_eq!(
            samples.iter().map(|s| s.value).collect::<Vec<_>>(),
            vec![Some(0.), None, None]
        );
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(bevy::ui_widgets::ButtonPlugin)
            .insert_resource(SidebarPage::Results)
            .insert_resource(data())
            .insert_resource(settings())
            .init_resource::<ProbePin>()
            .init_resource::<Comparison>()
            .init_resource::<ResultGeometry>()
            .init_resource::<FemModelVersion>()
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(spawn);
            })
            .add_systems(Update, update);
        app.update();
        app
    }
    fn click(app: &mut App, matches: impl Fn(&Action) -> bool) {
        let entity = app
            .world_mut()
            .query::<(Entity, &Action)>()
            .iter(app.world())
            .find(|(_, a)| matches(a))
            .unwrap()
            .0;
        crate::widget_test_input::click(app.world_mut(), entity);
        app.update();
    }
    fn pin(app: &mut App, index: usize) {
        app.insert_resource(ProbePin::for_test(0, target(index)));
        app.update();
    }

    #[test]
    fn pointer_activation_adds_only_explicitly_and_individual_clear_keeps_pin() {
        let mut app = app();
        click(&mut app, |a| matches!(a, Action::Add)); // disabled without a pin
        assert!(app.world().resource::<Comparison>().entries.is_empty());
        pin(&mut app, 0);
        click(&mut app, |a| matches!(a, Action::Add));
        pin(&mut app, 1);
        assert_eq!(app.world().resource::<Comparison>().entries.len(), 1);
        click(&mut app, |a| matches!(a, Action::Add));
        click(&mut app, |a| matches!(a, Action::Add));
        assert!(
            app.world()
                .resource::<Comparison>()
                .status
                .contains("Already")
        );
        click(&mut app, |a| matches!(a, Action::Remove(0)));
        assert_eq!(app.world().resource::<Comparison>().entries[0].slot, 1);
        assert_eq!(
            app.world().resource::<ProbePin>().selection().unwrap().2,
            target(1)
        );
        app.world_mut().resource_mut::<ProbePin>().clear();
        app.update();
        assert_eq!(app.world().resource::<Comparison>().entries.len(), 1);
        click(&mut app, |a| matches!(a, Action::Clear));
        assert!(app.world().resource::<Comparison>().entries.is_empty());
    }

    #[test]
    fn comparison_resets_on_incompatible_field_page_or_geometry_but_not_playback() {
        let mut app = app();
        pin(&mut app, 0);
        click(&mut app, |a| matches!(a, Action::Add));
        for frame in [1, 2, 0] {
            app.world_mut()
                .resource_mut::<VisualizationSettings>()
                .contour
                .as_mut()
                .unwrap()
                .step_index = frame;
            app.update();
            assert_eq!(app.world().resource::<Comparison>().entries.len(), 1);
        }
        for steps in &mut app.world_mut().resource_mut::<FemResultSet>().by_mesh {
            for step in steps {
                step.fields.push(ResultField::NodeScalar {
                    name: "Q".into(),
                    values: vec![99.; 5],
                    min: 99.,
                    max: 99.,
                });
            }
        }
        app.world_mut()
            .resource_mut::<VisualizationSettings>()
            .contour
            .as_mut()
            .unwrap()
            .field_name = "Q".into();
        app.update();
        assert_eq!(app.world().resource::<Comparison>().entries.len(), 1);
        assert_eq!(app.world().resource::<Comparison>().field, "Q");
        app.world_mut()
            .resource_mut::<VisualizationSettings>()
            .contour
            .as_mut()
            .unwrap()
            .field_name = "missing".into();
        app.update();
        assert!(app.world().resource::<Comparison>().entries.is_empty());
        assert!(
            app.world()
                .resource::<Comparison>()
                .status
                .contains("matching this field")
        );
        app.insert_resource(settings());
        app.update();
        click(&mut app, |a| matches!(a, Action::Add));
        app.world_mut().resource_mut::<ResultGeometry>().visible = true;
        app.update();
        assert!(app.world().resource::<Comparison>().entries.is_empty());
        click(&mut app, |a| matches!(a, Action::Add));
        app.insert_resource(SidebarPage::Model);
        app.update();
        assert!(app.world().resource::<Comparison>().entries.is_empty());
    }

    #[test]
    fn focused_enter_adds_the_current_pin_via_widget_activation() {
        use bevy::input_focus::{FocusCause, InputFocus};
        let mut app = app();
        app.init_resource::<InputFocus>();
        crate::widget_test_input::enable_keyboard(&mut app);
        pin(&mut app, 0);
        let entity = app
            .world_mut()
            .query::<(Entity, &Action)>()
            .iter(app.world())
            .find(|(_, a)| matches!(a, Action::Add))
            .unwrap()
            .0;
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(entity, FocusCause::Navigated);
        crate::widget_test_input::key(&mut app, KeyCode::Enter);
        assert_eq!(app.world().resource::<Comparison>().entries.len(), 1);
    }
}
