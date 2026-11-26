use super::*;
use crate::{ButtonInteraction, InteractionButton};
use fem_core::FrontistrPartitionMethod as Method;
use hecmw::partition_control::rcb_split_count;
use std::num::NonZeroU16;

#[derive(Clone, Copy)]
enum Tool {
    Partitioner,
    Launcher,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Self::Partitioner => "hecmw_part1",
            Self::Launcher => "MPI launcher",
        }
    }

    fn path(self, run: &mut FrontistrRunState) -> &mut Option<PathBuf> {
        match self {
            Self::Partitioner => &mut run.partitioner,
            Self::Launcher => &mut run.mpi_launcher,
        }
    }
}

#[derive(Component)]
pub(crate) struct MpiToolButton {
    tool: Tool,
    reset: bool,
}

#[derive(Component)]
pub(crate) struct MpiTools;

#[derive(Component)]
pub(crate) struct MpiToolPath(Tool);

#[derive(Component)]
pub(crate) struct MpiPartitionSummary;

#[derive(Component, Clone, Copy)]
pub(crate) enum PartitionAction {
    Pmetis,
    Kmetis,
    Rcb,
    CycleAxis(usize),
}

impl PartitionAction {
    fn selected(self, method: &Method) -> bool {
        matches!(
            (self, method),
            (Self::Pmetis, Method::Pmetis)
                | (Self::Kmetis, Method::Kmetis)
                | (Self::Rcb, Method::Rcb(_))
        )
    }
}

#[derive(Component)]
pub(crate) struct PartitionActionLabel;

pub(super) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            },
            MpiTools,
        ))
        .with_children(|section| {
            for (tool, label) in [
                (Tool::Partitioner, "Choose hecmw_part1..."),
                (Tool::Launcher, "Choose mpiexec / mpirun..."),
            ] {
                section
                    .spawn(Node {
                        column_gap: px(4),
                        ..default()
                    })
                    .with_children(|row| {
                        button(
                            row,
                            MpiToolButton { tool, reset: false },
                            crate::ui_font::ButtonIcon::Open,
                            label,
                        );
                        button(
                            row,
                            MpiToolButton { tool, reset: true },
                            crate::ui_font::ButtonIcon::Refresh,
                            "Auto",
                        );
                    });
                section.spawn((
                    Text::default(),
                    TextFont {
                        font_size: FontSize::Px(9.5),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                    MpiToolPath(tool),
                ));
            }
            section.spawn((
                Text::new("PARTITION METHOD"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
            section
                .spawn(Node {
                    column_gap: px(4),
                    ..default()
                })
                .with_children(|row| {
                    for (action, label) in [
                        (PartitionAction::Pmetis, "PMETIS"),
                        (PartitionAction::Kmetis, "KMETIS"),
                        (PartitionAction::Rcb, "RCB"),
                    ] {
                        partition_button(row, action, label);
                    }
                });
            section.spawn((
                Text::default(),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                MpiPartitionSummary,
            ));
            section
                .spawn(Node {
                    column_gap: px(4),
                    row_gap: px(4),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|row| {
                    for index in 0..(u16::BITS as usize - 1) {
                        partition_button(row, PartitionAction::CycleAxis(index), "");
                    }
                });
        });
}

fn button(
    parent: &mut ChildSpawnerCommands,
    action: impl Component,
    icon: crate::ui_font::ButtonIcon,
    label: &str,
) {
    parent
        .spawn((
            InteractionButton,
            Node {
                flex_grow: 1.0,
                height: px(24),
                padding: UiRect::axes(px(4), px(0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            action,
        ))
        .with_children(|button| {
            crate::ui_font::button_label(button, icon, label, 10.0, TEXT_MAIN);
        });
}

fn partition_button(parent: &mut ChildSpawnerCommands, action: PartitionAction, label: &str) {
    parent
        .spawn((
            InteractionButton,
            Node {
                display: if matches!(action, PartitionAction::CycleAxis(_)) {
                    Display::None
                } else {
                    Display::Flex
                },
                min_width: px(36),
                flex_grow: 1.0,
                height: px(24),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PANEL_BORDER),
            action,
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(TEXT_MAIN),
            PartitionActionLabel,
        ));
}

pub(crate) fn update_mpi_tools(
    mut run: ResMut<FrontistrRunState>,
    mut buttons: Query<(&MpiToolButton, Ref<ButtonInteraction>, &mut BackgroundColor)>,
    mut sections: Query<&mut Node, With<MpiTools>>,
    mut paths: Query<(&MpiToolPath, &mut Text)>,
) {
    let visible = run.launch_mode == SolverLaunchMode::Mpi;
    for mut node in &mut sections {
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let enabled = visible && !run.is_running();
    for (button, interaction, mut background) in &mut buttons {
        if enabled && *interaction == ButtonInteraction::Pressed && interaction.is_changed() {
            if button.reset {
                *button.tool.path(&mut run) = None;
            } else {
                let dialog =
                    rfd::FileDialog::new().set_title(format!("Choose {}", button.tool.name()));
                #[cfg(windows)]
                let dialog = dialog.add_filter("Executable", &["exe"]);
                if let Some(path) = dialog.add_filter("All files", &["*"]).pick_file() {
                    *button.tool.path(&mut run) = Some(path);
                }
            }
        }
        background.set_if_neq(BackgroundColor(ordinary_button_color(
            *interaction,
            enabled,
        )));
    }
    for (path, mut label) in &mut paths {
        let (selected, automatic) = match path.0 {
            Tool::Partitioner => (&run.partitioner, "beside fistr1, then PATH"),
            Tool::Launcher => (&run.mpi_launcher, "mpiexec / mpirun on PATH"),
        };
        let detail = selected.as_ref().map_or_else(
            || format!("Auto ({automatic})"),
            |path| path.display().to_string(),
        );
        label.set_if_neq(Text::new(format!("{}: {detail}", path.0.name())));
    }
}

pub(crate) fn update_mpi_partition(
    run: Res<FrontistrRunState>,
    mut setup: ResMut<AnalysisSetup>,
    mut buttons: Query<(
        &PartitionAction,
        Ref<ButtonInteraction>,
        &mut BackgroundColor,
        &mut Node,
        &Children,
    )>,
    mut labels: Query<&mut Text, With<PartitionActionLabel>>,
    mut summaries: Query<&mut Text, (With<MpiPartitionSummary>, Without<PartitionActionLabel>)>,
) {
    let enabled = run.launch_mode == SolverLaunchMode::Mpi && !run.is_running();
    let ranks = run.mpi_ranks;
    let split_count = rcb_split_count(ranks);
    let current = setup.frontistr_partition.as_ref();
    let pressed = if enabled {
        buttons
            .iter()
            .find_map(|(action, interaction, _, node, _)| {
                (node.display != Display::None
                    && *interaction == ButtonInteraction::Pressed
                    && interaction.is_changed())
                .then_some(*action)
            })
    } else {
        None
    };
    if enabled
        && (pressed.is_some() || current.is_some_and(|settings| settings.domains.get() != ranks))
    {
        let mut settings = current.cloned().unwrap_or_default();
        if let Some(ranks) = NonZeroU16::new(ranks) {
            settings.domains = ranks;
        }
        match pressed {
            Some(PartitionAction::Pmetis) => settings.method = Method::Pmetis,
            Some(PartitionAction::Kmetis) => settings.method = Method::Kmetis,
            Some(PartitionAction::Rcb) if !matches!(settings.method, Method::Rcb(_)) => {
                settings.method = Method::Rcb(Vec::new());
            }
            _ => {}
        }
        if let Method::Rcb(axes) = &mut settings.method
            && let Some(count) = split_count
        {
            axes.truncate(count);
            axes.extend((axes.len()..count).map(|index| ['x', 'y', 'z'][index % 3]));
            if let Some(PartitionAction::CycleAxis(index)) = pressed
                && let Some(axis) = axes.get_mut(index)
            {
                *axis = match *axis {
                    'x' => 'y',
                    'y' => 'z',
                    _ => 'x',
                };
            }
        }
        if current != Some(&settings) {
            setup.frontistr_partition = Some(settings);
        }
    }

    let (kind, method) =
        setup
            .frontistr_partition
            .as_ref()
            .map_or(("NODE-BASED", &Method::Pmetis), |settings| {
                let kind = if settings.element_based {
                    "ELEMENT-BASED"
                } else {
                    "NODE-BASED"
                };
                (kind, &settings.method)
            });
    let axes = match method {
        Method::Rcb(axes) if split_count.is_some() => axes.as_slice(),
        _ => &[],
    };
    for (action, interaction, mut background, mut node, children) in &mut buttons {
        if let PartitionAction::CycleAxis(index) = *action {
            let axis = axes.get(index);
            let display = if axis.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
            if let Some(axis) = axis
                && let Some(child) = children.first()
                && let Ok(mut text) = labels.get_mut(*child)
            {
                text.set_if_neq(Text::new(format!(
                    "{}: {}",
                    index + 1,
                    axis.to_ascii_uppercase()
                )));
            }
        }
        let color = if enabled && action.selected(method) && *interaction == ButtonInteraction::None
        {
            BUTTON_ACTIVE
        } else {
            ordinary_button_color(*interaction, enabled)
        };
        background.set_if_neq(BackgroundColor(color));
    }
    for mut summary in &mut summaries {
        let detail = match (method, split_count) {
            (Method::Rcb(_), None) => "\nRCB requires a power-of-two rank count (1, 2, 4, 8, ...).",
            (Method::Rcb(_), Some(0)) => "",
            (Method::Rcb(_), Some(_)) => "\nRCB axes in split order. Click to cycle X / Y / Z.",
            _ => "",
        };
        summary.set_if_neq(Text::new(format!("{kind} | DOMAIN={ranks}{detail}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press_partition_button(app: &mut App, matches: impl Fn(PartitionAction) -> bool) {
        let button = app
            .world_mut()
            .query::<(Entity, &PartitionAction)>()
            .iter(app.world())
            .find_map(|(entity, action)| matches(*action).then_some(entity))
            .unwrap();
        *app.world_mut()
            .get_mut::<ButtonInteraction>(button)
            .unwrap() = ButtonInteraction::Pressed;
        app.update();
        *app.world_mut()
            .get_mut::<ButtonInteraction>(button)
            .unwrap() = ButtonInteraction::None;
        app.update();
    }

    #[test]
    fn partition_controls_preserve_imported_options_and_write_all_three_methods() {
        let mut app = App::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hecmw_part_ctrl.dat");
        std::fs::write(&path, "!PARTITION, TYPE=NODE-BASED, METHOD=KMETIS, DOMAIN=8, DEPTH=2, UCD=cavityflow.inp, CONTACT=DISTRIBUTE, CONTACT_OWNER=SLAVE\n").unwrap();
        let partition = hecmw::partition_control::load(&path).unwrap();
        app.insert_resource(AnalysisSetup {
            frontistr_partition: Some(partition.clone()),
            ..default()
        });
        app.insert_resource(FrontistrRunState {
            launch_mode: SolverLaunchMode::Mpi,
            mpi_ranks: 8,
            ..default()
        });
        app.add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Node::default()).with_children(spawn);
        });
        app.add_systems(Update, update_mpi_partition);
        app.update();

        let model = FemModel::demo_hex8();
        let write = |app: &App| {
            let run = app.world().resource::<FrontistrRunState>();
            let setup = app.world().resource::<AnalysisSetup>();
            hecmw::write_parallel_frontistr_project(
                dir.path(),
                "test",
                &model,
                setup,
                "test_part",
                run.mpi_ranks,
            )
        };
        write(&app).unwrap();
        assert_eq!(hecmw::partition_control::load(&path).unwrap(), partition);

        press_partition_button(&mut app, |action| matches!(action, PartitionAction::Pmetis));
        write(&app).unwrap();
        assert_eq!(
            hecmw::partition_control::load(&path).unwrap(),
            fem_core::FrontistrPartitionSettings {
                method: Method::Pmetis,
                ..partition.clone()
            }
        );

        press_partition_button(&mut app, |action| matches!(action, PartitionAction::Rcb));
        press_partition_button(&mut app, |action| {
            matches!(action, PartitionAction::CycleAxis(0))
        });
        write(&app).unwrap();
        let rcb = hecmw::partition_control::load(&path).unwrap();
        assert_eq!(
            rcb,
            fem_core::FrontistrPartitionSettings {
                method: Method::Rcb(vec!['y', 'y', 'z']),
                ..partition.clone()
            }
        );
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .ends_with("\ny,y,z\n")
        );

        app.world_mut()
            .resource_mut::<FrontistrRunState>()
            .mpi_ranks = 3;
        app.update();
        assert!(
            write(&app)
                .unwrap_err()
                .to_string()
                .contains("power-of-two")
        );
        assert_eq!(hecmw::partition_control::load(&path).unwrap(), rcb);

        app.world_mut()
            .resource_mut::<FrontistrRunState>()
            .mpi_ranks = 4;
        app.update();
        write(&app).unwrap();
        assert_eq!(
            hecmw::partition_control::load(&path).unwrap(),
            fem_core::FrontistrPartitionSettings {
                method: Method::Rcb(vec!['y', 'y']),
                domains: NonZeroU16::new(4).unwrap(),
                ..partition
            }
        );
    }

    #[test]
    fn auto_resets_only_the_selected_mpi_tool_and_is_inactive_in_direct_mode() {
        let mut app = App::new();
        app.init_resource::<fem_core::AnalysisSetup>();
        app.insert_resource(FrontistrRunState {
            launch_mode: SolverLaunchMode::Direct,
            partitioner: Some("chosen-partitioner".into()),
            mpi_launcher: Some("chosen-launcher".into()),
            ..default()
        });
        let reset = app
            .world_mut()
            .spawn((
                MpiToolButton {
                    tool: Tool::Partitioner,
                    reset: true,
                },
                ButtonInteraction::Pressed,
                BackgroundColor::default(),
            ))
            .id();
        let section = app.world_mut().spawn((MpiTools, Node::default())).id();
        app.add_systems(Update, update_mpi_tools);
        app.update();
        assert_eq!(
            app.world().get::<Node>(section).unwrap().display,
            Display::None
        );
        assert!(
            app.world()
                .resource::<FrontistrRunState>()
                .partitioner
                .is_some()
        );

        app.world_mut()
            .resource_mut::<FrontistrRunState>()
            .launch_mode = SolverLaunchMode::Mpi;
        *app.world_mut().get_mut::<ButtonInteraction>(reset).unwrap() = ButtonInteraction::None;
        app.update();
        *app.world_mut().get_mut::<ButtonInteraction>(reset).unwrap() = ButtonInteraction::Pressed;
        app.update();
        assert_eq!(
            app.world().get::<Node>(section).unwrap().display,
            Display::Flex
        );
        let prefs = app.world().resource::<FrontistrRunState>().preferences();
        assert!(prefs.partitioner.is_none());
        assert_eq!(prefs.mpi_launcher, Some("chosen-launcher".into()));
    }
}
