//! Discrete solver choices: click to commit, native menu dismissal to cancel.
use bevy::ui_widgets::ScrollArea;
use crate::InteractionButton;
use crate::layout::{SidebarPage, UiInputCapture};
use bevy::{
    input_focus::{
        FocusCause, InputFocus,
        tab_navigation::{NavAction, TabIndex},
    },
    picking::{Pickable, hover::Hovered},
    prelude::*,
    ui::{ScrollPosition, Selected},
    ui_widgets::{
        ActiveDescendant, Button as WidgetButton, ListItem, MenuAction, MenuButton,
        MenuEvent, MenuFocusState, MenuPopup, ValueChange,
        popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
    },
};
use fem_core::{
    AnalysisSetup, AnalysisType, LinearSolverMethod, MatrixDumpFormat, SolverSettings, SolverTimeLog,
};

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Analysis,
    Method,
    Preconditioner,
    Scaling,
    ResidualLog,
    TimingLog,
    MatrixDump,
    DumpExit,
}
#[derive(Component)]
struct Label(Kind);
#[derive(Component)]
struct Selector;
#[derive(Component)]
struct SolverChoices;
#[derive(Component)]
struct Popup {
    original: SolverSettings,
}
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Analysis(AnalysisType),
    Method(LinearSolverMethod),
    Preconditioner(u8),
    Scaling(Option<bool>),
    ResidualLog(Option<bool>),
    TimingLog(Option<SolverTimeLog>),
    MatrixDump(MatrixDumpFormat),
    DumpExit(bool),
}

impl Kind {
    fn title(self) -> &'static str {
        match self {
            Self::Analysis => "Analysis type",
            Self::Method => "Linear solver",
            Self::Preconditioner => "Preconditioner",
            Self::Scaling => "Matrix scaling",
            Self::ResidualLog => "Residual log",
            Self::TimingLog => "Solver timing log",
            Self::MatrixDump => "Matrix dump",
            Self::DumpExit => "Dump action",
        }
    }
    fn available(self, settings: &SolverSettings) -> bool {
        !matches!(self, Self::Preconditioner | Self::Scaling) || settings.solver_method.is_iterative()
    }
    fn current(self, settings: &SolverSettings) -> Choice {
        match self {
            Self::Analysis => Choice::Analysis(settings.analysis_type),
            Self::Method => Choice::Method(settings.solver_method),
            Self::Preconditioner => Choice::Preconditioner(settings.preconditioner),
            Self::Scaling => Choice::Scaling(settings.matrix_scaling),
            Self::ResidualLog => Choice::ResidualLog(settings.iteration_log),
            Self::TimingLog => Choice::TimingLog(settings.time_log),
            Self::MatrixDump => Choice::MatrixDump(settings.matrix_dump_format),
            Self::DumpExit => Choice::DumpExit(settings.exit_after_matrix_dump),
        }
    }
    fn choices(self) -> Vec<Choice> {
        match self {
            Self::Analysis => [
                AnalysisType::Static,
                AnalysisType::NlStatic,
                AnalysisType::Dynamic,
                AnalysisType::Eigen,
            ]
            .map(Choice::Analysis)
            .to_vec(),
            Self::Method => [
                LinearSolverMethod::Mumps,
                LinearSolverMethod::DirectMkl,
                LinearSolverMethod::Cg,
                LinearSolverMethod::BiCgStab,
                LinearSolverMethod::Gmres,
                LinearSolverMethod::Direct,
            ]
            .map(Choice::Method)
            .to_vec(),
            Self::Preconditioner => [1, 2, 3].map(Choice::Preconditioner).to_vec(),
            Self::Scaling => [None, Some(false), Some(true)].map(Choice::Scaling).to_vec(),
            Self::ResidualLog => [None, Some(false), Some(true)]
                .map(Choice::ResidualLog)
                .to_vec(),
            Self::TimingLog => [
                None,
                Some(SolverTimeLog::Off),
                Some(SolverTimeLog::Summary),
                Some(SolverTimeLog::Verbose),
            ]
            .map(Choice::TimingLog)
            .to_vec(),
            Self::MatrixDump => [
                MatrixDumpFormat::None,
                MatrixDumpFormat::MatrixMarket,
                MatrixDumpFormat::Csr,
                MatrixDumpFormat::Bsr,
            ]
            .map(Choice::MatrixDump)
            .to_vec(),
            Self::DumpExit => vec![Choice::DumpExit(false), Choice::DumpExit(true)],
        }
    }
}
impl Choice {
    fn label(self) -> &'static str {
        match self {
            Self::Analysis(a) => a.label(),
            Self::Method(m) => m.label(),
            Self::Preconditioner(1) => "SSOR (1)",
            Self::Preconditioner(2) => "SSOR (2)",
            Self::Preconditioner(3) => "Diagonal scaling (3)",
            Self::Preconditioner(_) => "Unsupported",
            Self::Scaling(None) | Self::ResidualLog(None) | Self::TimingLog(None) => "Default",
            Self::Scaling(Some(false))
            | Self::ResidualLog(Some(false))
            | Self::TimingLog(Some(SolverTimeLog::Off)) => "Off",
            Self::Scaling(Some(true)) | Self::ResidualLog(Some(true)) => "On",
            Self::TimingLog(Some(SolverTimeLog::Summary)) => "Summary",
            Self::TimingLog(Some(SolverTimeLog::Verbose)) => "Verbose",
            Self::MatrixDump(MatrixDumpFormat::None) => "Off",
            Self::MatrixDump(MatrixDumpFormat::MatrixMarket) => "Matrix Market (MM)",
            Self::MatrixDump(format) => format.frontistr_value(),
            Self::DumpExit(false) => "Continue solve",
            Self::DumpExit(true) => "Exit after dump",
        }
    }
    fn keyword(self) -> String {
        match self {
            Self::Analysis(AnalysisType::NlStatic) => "!SOLUTION, TYPE=STATIC, NONLINEAR".into(),
            Self::Analysis(a) => format!("!SOLUTION, TYPE={}", a.frontistr_type()),
            Self::Method(m) => format!("!SOLVER, METHOD={}", m.frontistr_method()),
            Self::Preconditioner(code) => format!("PRECOND={code}"),
            Self::Scaling(None) => "!SOLVER without SCALING".into(),
            Self::Scaling(Some(false)) => "SCALING=NO".into(),
            Self::Scaling(Some(true)) => "SCALING=YES".into(),
            Self::ResidualLog(None) => "!SOLVER without ITERLOG".into(),
            Self::ResidualLog(Some(false)) => "ITERLOG=NO".into(),
            Self::ResidualLog(Some(true)) => "ITERLOG=YES".into(),
            Self::TimingLog(None) => "!SOLVER without TIMELOG".into(),
            Self::TimingLog(Some(setting)) => format!("TIMELOG={}", setting.frontistr_value()),
            Self::MatrixDump(format) => format!("!SOLVER, DUMPTYPE={}", format.frontistr_value()),
            Self::DumpExit(false) => "!SOLVER, DUMPEXIT=NO".into(),
            Self::DumpExit(true) => "!SOLVER, DUMPEXIT=YES".into(),
        }
    }
    fn selected(self, settings: &SolverSettings) -> bool {
        match self {
            Self::Analysis(a) => settings.analysis_type == a,
            Self::Method(m) => settings.solver_method == m,
            Self::Preconditioner(code) => settings.preconditioner == code,
            Self::Scaling(setting) => settings.matrix_scaling == setting,
            Self::ResidualLog(setting) => settings.iteration_log == setting,
            Self::TimingLog(setting) => settings.time_log == setting,
            Self::MatrixDump(format) => settings.matrix_dump_format == format,
            Self::DumpExit(exit) => settings.exit_after_matrix_dump == exit,
        }
    }
}
fn text(value: impl Into<String>) -> impl Bundle {
    (
        Text::new(value),
        Pickable::IGNORE,
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(Color::WHITE),
    )
}

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    for kind in [
        Kind::Analysis,
        Kind::Method,
        Kind::Preconditioner,
        Kind::Scaling,
        Kind::ResidualLog,
        Kind::TimingLog,
        Kind::MatrixDump,
        Kind::DumpExit,
    ] {
        parent
            .spawn((
                kind,
                Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
            ))
            .observe(menu_event)
            .with_children(|anchor| {
                anchor
                    .spawn((
                        InteractionButton,
                        WidgetButton,
                        MenuButton,
                        Selector,
                        TabIndex(0),
                        crate::popup_trigger::node(),
                        crate::popup_trigger::bundle(),
                        BackgroundColor(Color::srgb(0.14, 0.30, 0.37)),
                    ))
                    .with_children(|button| crate::popup_trigger::content(button,
                        (text(kind.title()), Label(kind)), "Click to choose a value. Enter opens; Esc closes."));
            });
    }
    parent.spawn(text(
        "Choose a value to apply. Ctrl+Z undoes. Launch Direct / MPI below is separate.",
    ));
}

fn menu_event(
    event: On<MenuEvent>,
    menus: Query<(Entity, &Kind, &Children)>,
    parents: Query<&ChildOf>,
    popups: Query<Entity, With<Popup>>,
    buttons: Query<(), With<Selector>>,
    setup: Res<AnalysisSetup>,
    page: Res<SidebarPage>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    let Some((anchor, kind, children)) = std::iter::once(event.source)
        .chain(parents.iter_ancestors(event.source))
        .find_map(|e| menus.get(e).ok())
    else {
        return;
    };
    let existing = children.iter().find(|e| popups.contains(*e));
    match event.action {
        MenuAction::CloseAll => {
            if let Some(e) = existing {
                commands.entity(e).despawn();
            }
        }
        MenuAction::FocusRoot => {
            if *page == SidebarPage::Solve && kind.available(&setup.solver) {
                if let Some(button) = children.iter().find(|e| buttons.contains(*e)) {
                    focus.set(button, FocusCause::Navigated);
                }
            }
        }
        MenuAction::Open(_) | MenuAction::Toggle => {
            if *page != SidebarPage::Solve || !kind.available(&setup.solver) {
                return;
            }
            if let Some(e) = existing {
                if matches!(event.action, MenuAction::Toggle) {
                    commands.entity(e).despawn();
                }
                return;
            }
            // Keep just one of our selectors open; other native menus also dismiss on focus loss.
            for e in &popups {
                commands.entity(e).despawn();
            }
            let nav = if let MenuAction::Open(nav) = event.action {
                nav
            } else {
                NavAction::First
            };
            commands
                .spawn((
                    ChildOf(anchor),
                    Popup {
                        original: setup.solver.clone(),
                    },
                    MenuPopup::default(),
                    MenuFocusState::Opening(nav),
                    GlobalZIndex(200),
                    OverrideClip,
                    UiInputCapture,
                    ScrollArea,
                    ScrollPosition::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(310),
                        max_width: Val::Vw(90.0),
                        max_height: Val::Vh(50.0),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        padding: UiRect::all(px(5)),
                        row_gap: px(3),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.07, 0.10, 0.12)),
                    Popover {
                        positions: vec![
                            PopoverPlacement {
                                side: PopoverSide::Bottom,
                                align: PopoverAlign::Start,
                                gap: 4.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Top,
                                align: PopoverAlign::Start,
                                gap: 4.0,
                            },
                        ],
                        window_margin: 10.0,
                    },
                ))
                .with_children(|popup| {
                    popup.spawn(text(format!(
                        "{} | click to apply; Esc cancels",
                        kind.title()
                    )));
                    crate::popup_select::list(popup, nav, |list| {
                        kind.choices().into_iter().map(|choice| {
                            let selected = choice.selected(&setup.solver);
                            let mut row = list.spawn((
                                ListItem,
                                choice,
                                Hovered::default(),
                                Node {
                                    min_height: px(40),
                                    flex_shrink: 0.0,
                                    padding: UiRect::all(px(5)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.10, 0.14, 0.17)),
                            ));
                            if selected {
                                row.insert(Selected);
                            }
                            row.with_child(text(format!(
                                "{} {}\n{}",
                                if selected { "[x]" } else { "[ ]" },
                                choice.label(),
                                choice.keyword()
                            )))
                            .id()
                        }).collect()
                    })
                    .insert(SolverChoices)
                    .observe(choose);
                });
        }
    }
}

fn choose(
    event: On<ValueChange<Entity>>,
    choices: Query<&Choice>,
    parents: Query<&ChildOf>,
    popups: Query<&Popup>,
    page: Res<SidebarPage>,
    mut setup: ResMut<AnalysisSetup>,
    mut commands: Commands,
) {
    crate::popup_select::dismiss(event.source, &mut commands);
    if *page != SidebarPage::Solve {
        return;
    }
    let Ok(choice) = choices.get(event.value) else {
        return;
    };
    let Some(popup) = parents
        .iter_ancestors(event.value)
        .find_map(|e| popups.get(e).ok())
    else {
        return;
    };
    // Do not apply an old menu over a project load, undo or numeric edit.
    if setup.solver != popup.original || choice.selected(&setup.solver) {
        return;
    }
    match *choice {
        Choice::Analysis(value) => setup.solver.analysis_type = value,
        Choice::Method(value) => setup.solver.solver_method = value,
        Choice::Preconditioner(code) => setup.solver.preconditioner = code,
        Choice::Scaling(setting) => setup.solver.matrix_scaling = setting,
        Choice::ResidualLog(setting) => setup.solver.iteration_log = setting,
        Choice::TimingLog(setting) => setup.solver.time_log = setting,
        Choice::MatrixDump(format) => setup.solver.matrix_dump_format = format,
        Choice::DumpExit(exit) => setup.solver.exit_after_matrix_dump = exit,
    }
}

pub(crate) fn register(app: &mut App) {
    app.add_systems(
        Update,
        sync.after(crate::layout::sidebar_page_button_system)
            .after(crate::layout::undo_redo_system)
            .after(crate::solver_editor::solver_numeric_input_system).in_set(crate::popup_trigger::MenuSync),
    );
}
fn sync(
    page: Res<SidebarPage>,
    setup: Res<AnalysisSetup>,
    mut commands: Commands,
    popups: Query<(Entity, &Popup)>,
    mut labels: Query<(&Label, &mut Text)>,
    mut anchors: Query<(&Kind, &Children, &mut Node)>,
    mut items: Query<(Entity, &Choice, &Hovered, &mut BackgroundColor)>,
    mut focus: ResMut<InputFocus>,
    lists: Query<&ActiveDescendant>,
    menu_focus: Query<(), Or<(With<Selector>, With<SolverChoices>)>>,
) {
    if *page != SidebarPage::Solve && focus.get().is_some_and(|e| menu_focus.contains(e)) {
        focus.clear();
    }
    for (entity, popup) in &popups {
        if *page != SidebarPage::Solve || setup.solver != popup.original {
            if *page == SidebarPage::Solve && focus.get().is_some_and(|e| menu_focus.contains(e)) {
                focus.clear();
            }
            commands.entity(entity).despawn();
        }
    }
    for (label, mut value) in &mut labels {
        value.set_if_neq(Text::new(format!(
            "{}: {}",
            label.0.title(),
            label.0.current(&setup.solver).label()
        )));
    }
    for (kind, children, mut anchor) in &mut anchors {
        let display = if kind.available(&setup.solver) { Display::Flex } else { Display::None };
        if anchor.display != display {
            anchor.display = display;
        }
        if display == Display::None && focus.get().is_some_and(|entity| children.contains(&entity)) {
            focus.clear();
        }
    }
    let active_row = crate::popup_select::active_row(&focus, &lists);
    for (entity, choice, hovered, mut color) in &mut items {
        *color = BackgroundColor(if hovered.0 || active_row == Some(entity) {
            Color::srgb(0.22, 0.40, 0.47)
        } else if choice.selected(&setup.solver) {
            Color::srgb(0.18, 0.45, 0.55)
        } else {
            Color::srgb(0.10, 0.14, 0.17)
        });
    }
}

#[cfg(test)]
#[path = "solver_menu_tests.rs"]
mod tests;
