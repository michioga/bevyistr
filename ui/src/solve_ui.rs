use crate::layout::{SidebarPage, SidebarPageContent};
use crate::project_io::{ExportButton, ExportStatusText, OpenSetupButton};
use crate::solver_editor::spawn_solver_exact_editor;
use bevy::prelude::*;

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);

#[derive(Component)]
pub(crate) struct AnalysisSetupStatsText;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AnalysisTypeButton(pub fem_core::AnalysisType);

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SolverMethodButton(pub fem_core::LinearSolverMethod);

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

            solver
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4.0),
                    ..default()
                },))
                .with_children(|row| {
                    for analysis_type in [
                        fem_core::AnalysisType::Static,
                        fem_core::AnalysisType::NlStatic,
                        fem_core::AnalysisType::Dynamic,
                        fem_core::AnalysisType::Eigen,
                    ] {
                        row.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: px(22.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            AnalysisTypeButton(analysis_type),
                            Name::new(format!("AnalysisType_{}", analysis_type.label())),
                        ))
                        .with_child((
                            Text::new(analysis_type.label()),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    }
                });

            solver
                .spawn((Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(4.0),
                    ..default()
                },))
                .with_children(|row| {
                    for method in [
                        fem_core::LinearSolverMethod::Mumps,
                        fem_core::LinearSolverMethod::Cg,
                        fem_core::LinearSolverMethod::Gmres,
                        fem_core::LinearSolverMethod::Direct,
                    ] {
                        row.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: px(22.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_NORMAL),
                            BorderColor::all(PANEL_BORDER),
                            SolverMethodButton(method),
                            Name::new(format!("SolverMethod_{}", method.label())),
                        ))
                        .with_child((
                            Text::new(method.label()),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(TEXT_MAIN),
                        ));
                    }
                });

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
}

pub(crate) fn analysis_type_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &AnalysisTypeButton,
        ),
        With<AnalysisTypeButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            setup.solver.analysis_type = button.0;
        }
        let active = setup.solver.analysis_type == button.0;
        *background = BackgroundColor(button_color(*interaction, active));
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn solver_method_button_system(
    mut setup: ResMut<fem_core::AnalysisSetup>,
    mut buttons: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &SolverMethodButton,
        ),
        With<SolverMethodButton>,
    >,
) {
    for (interaction, mut background, mut border, button) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            setup.solver.solver_method = button.0;
        }
        let active = setup.solver.solver_method == button.0;
        *background = BackgroundColor(button_color(*interaction, active));
        *border = BorderColor::all(PANEL_BORDER);
    }
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

fn button_color(interaction: Interaction, active: bool) -> Color {
    match (interaction, active) {
        (Interaction::Pressed, _) => BUTTON_PRESSED,
        (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
        (Interaction::Hovered, false) => BUTTON_HOVERED,
        (Interaction::None, false) => BUTTON_NORMAL,
    }
}
