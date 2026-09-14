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
}
