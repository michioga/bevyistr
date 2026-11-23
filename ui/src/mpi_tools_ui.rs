use crate::{ButtonInteraction, InteractionButton};
use super::*;

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
                Text::new("MPI ranks set DOMAIN. Partition: NODE-BASED / PMETIS."),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                MpiPartitionSummary,
            ));
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

pub(crate) fn update_mpi_tools(
    mut run: ResMut<FrontistrRunState>,
    mut buttons: Query<(&MpiToolButton, Ref<ButtonInteraction>, &mut BackgroundColor)>,
    mut sections: Query<&mut Node, With<MpiTools>>,
    mut paths: Query<(&MpiToolPath, &mut Text), Without<MpiPartitionSummary>>,
    mut summaries: Query<&mut Text, With<MpiPartitionSummary>>,
    setup: Res<fem_core::AnalysisSetup>,
) {
    if setup.is_changed() {
        let (kind, method) = setup.frontistr_partition.as_ref().map_or(
            ("NODE-BASED", "PMETIS"),
            |settings| (
                if settings.element_based { "ELEMENT-BASED" } else { "NODE-BASED" },
                settings.method.keyword(),
            ),
        );
        for mut summary in &mut summaries {
            summary.set_if_neq(Text::new(format!("MPI ranks set DOMAIN. Partition: {kind} / {method}.")));
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
