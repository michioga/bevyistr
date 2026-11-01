use crate::layout::{SidebarPage, SidebarPageContent};
use crate::project_io::{ExportButton, ExportStatusText, OpenSetupButton};
use crate::solver_editor::spawn_solver_exact_editor;
use crate::solver_runner::spawn_solver_execution_ui;
use bevy::prelude::*;

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);

#[derive(Component)]
pub(crate) struct AnalysisSetupStatsText;

#[derive(Component)]
pub(crate) struct SetupImportWarningText;

/// Fixed, cross-page notice: opening a project must not hide import blockers
/// until the user happens to visit Solve or tries to run it.
pub(crate) fn spawn_import_warning(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(10.5),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.72, 0.35)),
        Node {
            display: Display::None,
            margin: UiRect::axes(px(10.0), px(4.0)),
            ..default()
        },
        SetupImportWarningText,
    ));
}

pub(crate) fn update_import_warning(
    setup: Res<fem_core::AnalysisSetup>,
    page: Res<SidebarPage>,
    mut query: Query<(&mut Text, &mut Node), With<SetupImportWarningText>>,
) {
    if !setup.is_changed() && !page.is_changed() {
        return;
    }
    for (mut text, mut node) in &mut query {
        node.display = if setup.import_issues.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        let viewing = if *page == SidebarPage::Results {
            "Saved result viewing is available."
        } else {
            "Mesh / result viewing is available."
        };
        let details = if *page == SidebarPage::Solve {
            "Details below."
        } else {
            "Details: Solve."
        };
        let count = setup.import_issues.len();
        let entry_label = if count == 1 { "entry" } else { "entries" };
        text.set_if_neq(Text::new(format!(
            "CNT re-export blocked: {count} {entry_label}.\nSettings not preserved by bevyistr.\n{viewing}\nExport / Run blocked. {details}"
        )));
    }
}

pub(crate) fn spawn_solve_ui(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(6.0),
                ..default()
            },
            SidebarPageContent::page(SidebarPage::Solve),
        ))
        .with_children(|row| {
            row.spawn((
                Button,
                Node {
                    flex_grow: 1.0,
                    height: px(28.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(5.0)),
                    ..default()
                },
                BackgroundColor(BUTTON_NORMAL),
                BorderColor::all(PANEL_BORDER),
                OpenSetupButton,
                Name::new("OpenSetupButton"),
            ))
            .with_child((
                Text::new("Open Setup"),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));

            row.spawn((
                Button,
                Node {
                    flex_grow: 1.0,
                    height: px(28.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(5.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.10, 0.32, 0.18)),
                BorderColor::all(Color::srgb(0.15, 0.50, 0.28)),
                ExportButton,
                Name::new("ExportButton"),
            ))
            .with_child((
                Text::new("Export"),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(Color::srgb(0.75, 0.97, 0.80)),
            ));
        });

    parent.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(10.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
        ExportStatusText,
        SidebarPageContent::page(SidebarPage::Solve),
    ));
    parent.spawn((
        Text::new("Setup: none loaded"),
        TextFont {
            font_size: FontSize::Px(11.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
        AnalysisSetupStatsText,
        SidebarPageContent::page(SidebarPage::Solve),
    ));

    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                margin: UiRect::top(px(6.0)),
                padding: UiRect::all(px(6.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(5.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(0.30, 0.36, 0.55, 0.50)),
            SidebarPageContent::page(SidebarPage::Solve),
            Name::new("SolverPanel"),
        ))
        .with_children(|solver| {
            solver.spawn((
                Text::new("Solver Settings"),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(Color::srgba(0.55, 0.65, 0.90, 0.90)),
            ));

            crate::solver_menu::spawn(solver);

            spawn_solver_exact_editor(solver);
            solver.spawn((
                Text::new("Settings written to !SOLUTION / !SOLVER in .cnt"),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(Color::srgba(0.45, 0.54, 0.60, 0.80)),
            ));
        });

    crate::output_ui::spawn(parent);
    spawn_solver_execution_ui(parent);
}

pub(crate) fn update_analysis_setup_stats_text(
    setup: Res<fem_core::AnalysisSetup>,
    mut query: Query<&mut Text, With<AnalysisSetupStatsText>>,
) {
    if !setup.is_changed() {
        return;
    }

    let Ok(mut text) = query.single_mut() else {
        return;
    };

    **text = if setup.is_empty() {
        "Setup: none loaded".to_string()
    } else {
        let constrained_nodes: usize = setup
            .boundary_conditions
            .iter()
            .map(|condition| condition.nodes.len())
            .sum();

        format!(
            "Setup: BC {} ({} nodes)  Loads {}  MPC {}  Materials {}  Sections {}",
            setup.boundary_conditions.len(),
            constrained_nodes,
            setup.nodal_loads.len() + setup.distributed_loads.len(),
            setup.mpc_equations.len(),
            setup.materials.len(),
            setup.sections.len(),
        )
    };
    if !setup.import_issues.is_empty() {
        text.0.push_str("\n\nCNT RE-EXPORT LIMITATION\nbevyistr cannot preserve all imported CNT settings.\nShown solver controls cover only the supported subset.\nEntries below identify conversion limits in this editor.\n");
        for issue in &setup.import_issues {
            text.0.push_str(&format!("\n{issue}\n"));
        }
        text.0.push_str("\nExport / Run blocked to prevent settings loss. Mesh and saved results remain viewable. Keep the original input for external runs; changing a dropdown does not resolve omitted settings.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_warning_is_visible_and_details_survive_solver_edits() {
        let mut app = App::new();
        app.init_resource::<fem_core::AnalysisSetup>()
            .init_resource::<SidebarPage>();
        app.add_systems(
            Update,
            (update_import_warning, update_analysis_setup_stats_text),
        );
        let notice = app
            .world_mut()
            .spawn((Text::new(""), Node::default(), SetupImportWarningText))
            .id();
        let details = app
            .world_mut()
            .spawn((Text::new(""), AnalysisSetupStatsText))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<Node>(notice).unwrap().display,
            Display::None
        );
        app.world_mut()
            .resource_mut::<fem_core::AnalysisSetup>()
            .import_issues
            .push(fem_core::SetupImportIssue {
                source: "heat.cnt".into(),
                line: 4,
                keyword: "!SOLUTION, TYPE=HEAT".into(),
                message: "Unsupported analysis".into(),
            });
        app.update();
        assert_eq!(
            app.world().get::<Node>(notice).unwrap().display,
            Display::Flex
        );
        assert!(
            app.world()
                .get::<Text>(details)
                .unwrap()
                .0
                .contains("heat.cnt:4")
        );
        let message = &app.world().get::<Text>(notice).unwrap().0;
        assert!(message.contains("CNT re-export blocked: 1 entry."));
        assert!(message.contains("Settings not preserved by bevyistr"));
        assert!(message.contains("Mesh / result viewing is available"));
        // Changing pages updates the explanation without clearing blockers or
        // implying that the loaded result failed.
        *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Results;
        app.update();
        let message = &app.world().get::<Text>(notice).unwrap().0;
        assert!(message.contains("Saved result viewing is available"));
        assert!(message.contains("Export / Run blocked"));
        assert!(message.contains("Details: Solve"));
        *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Solve;
        app.update();
        assert!(
            app.world()
                .get::<Text>(notice)
                .unwrap()
                .0
                .contains("Details below")
        );
        assert_eq!(
            app.world()
                .resource::<fem_core::AnalysisSetup>()
                .import_issues
                .len(),
            1
        );
        app.world_mut()
            .resource_mut::<fem_core::AnalysisSetup>()
            .solver
            .analysis_type = fem_core::AnalysisType::Eigen;
        app.update();
        assert!(
            app.world()
                .get::<Text>(details)
                .unwrap()
                .0
                .contains("TYPE=HEAT")
        );
        app.world_mut()
            .resource_mut::<fem_core::AnalysisSetup>()
            .clear();
        app.update();
        assert_eq!(
            app.world().get::<Node>(notice).unwrap().display,
            Display::None
        );
    }
}
